//! An interface to the Headless Chrome

#![allow(dead_code)]

use serde::Serialize;
use hyper::client::{Client, Connect, FutureResponse};
use websocket::WebSocketResult;
use websocket::message::OwnedMessage;
use websocket::sender::Writer as WebSocketWriter;
use websocket::receiver::Reader as WebSocketReader;
use websocket::client::ClientBuilder;
use std::process::{Child, Command};
use std::io::prelude::{Write, Read};
use std::net::TcpStream;
use std::io::{Result as IOResult, ErrorKind as IOErrorKind};
use serde_json::{Value as JValue, Map as JMap}; use serde_json;
use GenericResult;

/// `json/version` response
#[derive(Deserialize)]
pub struct BrowserVersion<'s>
{
	#[serde(rename = "Protocol-Version")]
	pub protocol_version: &'s str,
	#[serde(rename = "WebKit-Version")]
	pub webkit_version: &'s str,
	#[serde(rename = "Browser")]
	pub browser: &'s str,
	#[serde(rename = "User-Agent")]
	pub user_agent: &'s str,
	#[serde(rename = "V8-Version")]
	pub v8_version: &'s str
}

struct DummyIterator;
impl Iterator for DummyIterator
{
	type Item = ::websocket::dataframe::DataFrame;
	fn next(&mut self) -> Option<Self::Item> { None }
}

/// Subscribes an event of `E`
pub trait SessionEventSubscriber<E: Event>
{
	fn on_event(&mut self, event: &E);
}
impl<F, E: Event> SessionEventSubscriber<E> for F where F: FnMut(&E)
{
	fn on_event(&mut self, event: &E) { self(event) }
}
/// Allows subscribing events
pub trait SessionEventSubscribable<E: Event>
{
	unsafe fn subscribe_session_event_raw(&mut self, subscriber: *mut SessionEventSubscriber<E>);
	unsafe fn unsubscribe_session_event_raw(&mut self, subscriber: *mut SessionEventSubscriber<E>);
	fn subscribe_session_event(&mut self, subscriber: &SessionEventSubscriber<E>)
	{
		unsafe { self.subscribe_session_event_raw(subscriber as *const _ as *mut _) }
	}
	fn unsubscribe_session_event(&mut self, subscriber: &SessionEventSubscriber<E>)
	{
		unsafe { self.unsubscribe_session_event_raw(subscriber as *const _ as *mut _) }
	}
}
pub trait Event: Sized
{
	const METHOD_NAME: &'static str;
	fn deserialize(res: JMap<String, JValue>) -> Self;
}

pub struct Session<W: Write, R: Read>
{
	sender: WebSocketWriter<W>, receiver: WebSocketReader<R>,
	frame_navigated_event_subscriber: Vec<*mut SessionEventSubscriber<page::FrameNavigated>>
}
impl Session<TcpStream, TcpStream>
{
	pub fn connect(addr: &str) -> GenericResult<Self>
	{
		let ws_client = ClientBuilder::new(addr)?.connect_insecure()?;
		let (recv, send) = ws_client.split()?;
		Ok(Session
		{
			sender: send, receiver: recv, frame_navigated_event_subscriber: Vec::new()
		})
	}
}
/// Session associated domains
impl<W: Write, R: Read> Session<W, R>
{
	pub fn dom(&mut self) -> domain::DOM<W, R> { domain::DOM(self) }
	pub fn input(&mut self) -> domain::Input<W, R> { domain::Input(self) }
	pub fn page(&mut self) -> domain::Page<W, R> { domain::Page(self) }
	pub fn runtime(&mut self) -> domain::Runtime<W, R> { domain::Runtime(self) }
}
macro_rules! SessionEventSubscribable
{
	($t: ty => $v: ident) =>
	{
		impl<W: Write, R: Read> SessionEventSubscribable<$t> for Session<W, R>
		{
			unsafe fn subscribe_session_event_raw(&mut self, subscriber: *mut SessionEventSubscriber<$t>)
			{
				self.$v.push(subscriber);
			}
			unsafe fn unsubscribe_session_event_raw(&mut self, subscriber: *mut SessionEventSubscriber<$t>)
			{
				if let Some(index) = self.$v.iter().position(|&x| x == subscriber) { self.$v.remove(index); }
			}
		}
	}
}
SessionEventSubscribable!(page::FrameNavigated => frame_navigated_event_subscriber);
impl<W: Write, R: Read> Session<W, R>
{
	pub fn wait_message(&mut self) -> WebSocketResult<OwnedMessage>
	{
		self.receiver.recv_message::<DummyIterator>()
	}
	pub fn wait_text(&mut self) -> WebSocketResult<String>
	{
		loop
		{
			match self.wait_message()?
			{
				OwnedMessage::Text(s) => return Ok(s),
				_ => ()
			}
		}
	}
	pub fn wait_event<E: Event>(&mut self) -> GenericResult<E>
	{
		loop
		{
			let s = self.wait_text()?;
			#[cfg(feature = "verbose")] println!("[wait_event]Received: {}", s);
			// let obj: HashMap<_, _> = ::json_flex::decode(s).unwrap();
			let mut parsed: JValue = serde_json::from_str(&s).unwrap();
			let obj = parsed.as_object_mut().unwrap();
			if Some(E::METHOD_NAME) == obj.get("method").and_then(JValue::as_str)
			{
				return Ok(E::deserialize(match obj.remove("params")
				{
					Some(JValue::Object(o)) => o, _ => api_corruption!(value_type)
				}));
			}
			if obj.contains_key("method") { self.handle_events(obj); }
			if let Some(e) = obj.get("error")
			{
				return Err(From::from(format!("RPC Error({}): {} in processing id {}", e["code"].as_i64().unwrap(),
					e["message"].as_str().unwrap(), obj["id"].as_u64().unwrap())));
			}
		}
	}
	pub fn wait_result(&mut self, id: usize) -> GenericResult<JValue>
	{
		loop
		{
			let s = self.wait_text()?;
			#[cfg(feature = "verbose")] println!("[wait_result]Received: {}", s);
			// let mut obj: HashMap<_, _> = ::json_flex::decode(s).unwrap();
			let mut parser: JValue = ::serde_json::from_str(&s).unwrap();
			let obj = parser.as_object_mut().unwrap();
			if obj.contains_key("result")
			{
				if obj["id"].as_u64() == Some(id as u64) { return Ok(obj.remove("result").unwrap()); }
			}
			if obj.contains_key("method") { self.handle_events(obj); }
			if let Some(e) = obj.get("error")
			{
				return Err(From::from(format!("RPC Error({}): {} in processing id {}", e["code"].as_i64().unwrap(),
					e["message"].as_str().unwrap(), obj["id"].as_u64().unwrap())));
			}
		}
	}
	fn handle_events(&mut self, obj: &mut JMap<String, JValue>)
	{
		if Some(page::FrameNavigated::METHOD_NAME) == obj.get("method").and_then(JValue::as_str)
		{
			let e = page::FrameNavigated::deserialize(match obj.remove("params")
			{
				Some(JValue::Object(o)) => o, _ => api_corruption!(value_type)
			});
			for &call in &self.frame_navigated_event_subscriber { unsafe { &mut *call }.on_event(&e); }
		}
	}
	fn send_text(&mut self, text: String) -> WebSocketResult<()>
	{
		// println!("Sending {}", text);
		#[cfg(feature = "verbose")] println!("[send]Sending: {}", text);
		self.sender.send_message(&OwnedMessage::Text(text))
	}
	fn send<T: Serialize>(&mut self, payload: &T) -> WebSocketResult<()>
	{
		self.send_text(::serde_json::to_string(payload).unwrap())
	}
}
pub mod dom
{
	use std::io::prelude::*;
	use serde_json::{Value as JValue, Map as JMap};

	pub struct DocumentUpdated;
	impl super::Event for DocumentUpdated
	{
		const METHOD_NAME: &'static str = "DOM.documentUpdated";
		fn deserialize(_: JMap<String, JValue>) -> Self { DocumentUpdated }
	}

	pub struct Node<'s, 'c: 's, W: Write + 'c, R: Read + 'c> { pub domain: &'s mut super::domain::DOM<'c, W, R>, pub id: isize }
	impl<'s, 'c: 's, W: Write + 'c, R: Read + 'c> Node<'s, 'c, W, R>
	{
		pub fn query_selector<'ss: 's>(&'ss mut self, selector: &str) -> super::GenericResult<Node<'s, 'c, W, R>>
		{
			self.domain.query_selector_sync(1000, self.id, selector).map(move |nid| Node { domain: self.domain, id: nid })
		}
		pub fn query_selector_all(&mut self, selector: &str) -> super::GenericResult<Vec<i64>>
		{
			self.domain.query_selector_all_sync(1000, self.id, selector).map(|v| v.into_iter().map(|x| x.as_i64().unwrap()).collect())
		}
		pub fn focus(&mut self) -> super::GenericResult<&mut Self>
		{
			self.domain.focus_sync(1000, self.id).map(move |_| self)
		}
		pub fn attributes(&mut self) -> super::GenericResult<Vec<JValue>>
		{
			self.domain.get_attributes_sync(1000, self.id).map(|v| match v
			{
				JValue::Object(mut o) => match o.remove("attributes")
				{
					Some(JValue::Array(v)) => v,
					_ => api_corruption!(value_type)
				},
				_ => api_corruption!(value_type)
			})
		}
	}
}
#[allow(dead_code)]
pub mod page
{
	use serde_json::{Value as JValue, Map as JMap};

	pub struct LoadEventFired { timestamp: f64 }
	impl super::Event for LoadEventFired
	{
		const METHOD_NAME: &'static str = "Page.loadEventFired";
		fn deserialize(res: JMap<String, JValue>) -> Self
		{
			LoadEventFired { timestamp: res["timestamp"].as_f64().unwrap() }
		}
	}
	pub struct FrameStoppedLoading { pub frame_id: String }
	impl super::Event for FrameStoppedLoading
	{
		const METHOD_NAME: &'static str = "Page.frameStoppedLoading";
		fn deserialize(mut res: JMap<String, JValue>) -> Self
		{
			FrameStoppedLoading
			{
				frame_id: match res.remove("frameId") { Some(JValue::String(s)) => s, _ => api_corruption!(value_type) }
			}
		}
	}
	pub struct FrameNavigated { pub frame_id: String, pub name: Option<String>, pub url: String }
	impl super::Event for FrameNavigated
	{
		const METHOD_NAME: &'static str = "Page.frameNavigated";
		fn deserialize(mut res: JMap<String, JValue>) -> Self
		{
			match res.remove("frame")
			{
				Some(JValue::Object(mut f)) => FrameNavigated
				{
					frame_id: match f.remove("id") { Some(JValue::String(s)) => s, _ => api_corruption!(value_type) },
					name: match f.remove("name") { Some(JValue::String(s)) => Some(s), None => None, _ => api_corruption!(value_type) },
					url: match f.remove("url") { Some(JValue::String(s)) => s, _ => api_corruption!(value_type) }
				},
				_ => api_corruption!(value_type)
			}
		}
	}
	impl Default for FrameNavigated
	{
		fn default() -> Self { FrameNavigated { frame_id: String::new(), name: None, url: String::new() } }
	}
}
#[allow(dead_code)]
pub mod input
{
	#[derive(Serialize)] #[serde(rename_all = "camelCase")]
	pub enum KeyEvent { KeyDown, KeyUp, RawKeyDown, Char }
}
pub mod runtime
{
	use serde_json::{Value as JValue, Map as JMap};

	pub struct ExecutionContextCreated { pub context_id: u64, pub name: String, pub aux: JValue }
	impl super::Event for ExecutionContextCreated
	{
		const METHOD_NAME: &'static str = "Runtime.executionContextCreated";
		fn deserialize(mut res: JMap<String, JValue>) -> Self
		{
			match res.remove("context")
			{
				Some(JValue::Object(mut ctx)) => ExecutionContextCreated
				{
					context_id: ctx["id"].as_u64().unwrap(),
					name: match ctx.remove("name") { Some(JValue::String(s)) => s, _ => api_corruption!(value_type) },
					aux: ctx.remove("auxData").unwrap()
				},
				_ => api_corruption!(value_type)
			}
		}
	}
	pub struct ExecutionContextDestroyed { pub context_id: u64 }
	impl super::Event for ExecutionContextDestroyed
	{
		const METHOD_NAME: &'static str = "Runtime.executionContextDestroyed";
		fn deserialize(res: JMap<String, JValue>) -> Self
		{
			ExecutionContextDestroyed { context_id: res["executionContextId"].as_u64().unwrap() }
		}
	}
	pub struct ExecutionContextsCleared;
	impl super::Event for ExecutionContextsCleared
	{
		const METHOD_NAME: &'static str = "Runtime.executionContextsCleared";
		fn deserialize(_: JMap<String, JValue>) -> Self { ExecutionContextsCleared }
	}
}
pub mod domain
{
	use super::Session;
	use std::io::prelude::*;
	use websocket::WebSocketResult;
	use serde_json::Value as JValue;

	pub struct DOM<'c, W: Write + 'c, R: Read + 'c>(pub &'c mut Session<W, R>);
	impl<'c, W: Write + 'c, R: Read + 'c> DOM<'c, W, R>
	{
		pub fn enable(&mut self, id: usize) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload { method: &'static str, id: usize }
			self.0.send(&Payload { method: "DOM.enable", id })
		}
		pub fn get_document(&mut self, id: usize) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload { method: &'static str, id: usize }
			self.0.send(&Payload { method: "DOM.getDocument", id })
		}
		pub fn query_selector(&mut self, id: usize, node_id: isize, selector: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: usize, params: Params<'s> }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { node_id: isize, selector: &'s str }
			self.0.send(&Payload { method: "DOM.querySelector", id, params: Params { node_id, selector } })
		}
		pub fn query_selector_all(&mut self, id: usize, node_id: isize, selector: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: usize, params: Params<'s> }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { node_id: isize, selector: &'s str }
			self.0.send(&Payload { method: "DOM.querySelectorAll", id, params: Params { node_id, selector } })
		}
		pub fn focus(&mut self, id: usize, node_id: isize) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload { method: &'static str, id: usize, params: Params }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params { node_id: isize }
			self.0.send(&Payload { method: "DOM.focus", id, params: Params { node_id } })
		}
		pub fn get_attributes(&mut self, id: usize, node_id: isize) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload { method: &'static str, id: usize, params: Params }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params { node_id: isize }
			self.0.send(&Payload { method: "DOM.getAttributes", id, params: Params { node_id } })
		}

		pub fn get_document_sync(&mut self, id: usize) -> super::GenericResult<JValue>
		{
			self.get_document(id).map_err(From::from).and_then(|_| self.0.wait_result(id))
		}
		pub fn get_root_node_sync<'s>(&'s mut self, id: usize) -> super::GenericResult<super::dom::Node<'s, 'c, W, R>>
		{
			self.get_document_sync(id).map(move |id| super::dom::Node { domain: self, id: id["root"]["nodeId"].as_i64().unwrap() as isize })
		}
		pub fn query_selector_sync(&mut self, id: usize, node_id: isize, selector: &str) -> super::GenericResult<isize>
		{
			self.query_selector(id, node_id, selector).map_err(From::from).and_then(|_| self.0.wait_result(id))
				.map(|o| o["nodeId"].as_i64().unwrap() as isize)
		}
		pub fn query_selector_all_sync(&mut self, id: usize, node_id: isize, selector: &str) -> super::GenericResult<Vec<JValue>>
		{
			self.query_selector_all(id, node_id, selector).map_err(From::from).and_then(|_| self.0.wait_result(id)).map(|o| match o
			{
				JValue::Object(mut o) => match o.remove("nodeIds")
				{
					Some(JValue::Array(v)) => v,
					_ => api_corruption!(value_type)
				},
				_ => api_corruption!(value_type)
			})
		}
		pub fn focus_sync(&mut self, id: usize, node_id: isize) -> super::GenericResult<()>
		{
			self.focus(id, node_id).map_err(From::from).and_then(|_| self.0.wait_result(id)).map(|_| ())
		}
		pub fn get_attributes_sync(&mut self, id: usize, node_id: isize) -> super::GenericResult<JValue>
		{
			self.get_attributes(id, node_id).map_err(From::from).and_then(|_| self.0.wait_result(id))
		}

		pub fn node_from<'s>(&'s mut self, id: isize) -> super::dom::Node<'s, 'c, W, R>
		{
			super::dom::Node { domain: self, id }
		}
	}
	pub struct Input<'c, W: Write + 'c, R: Read + 'c>(pub &'c mut Session<W, R>);
	impl<'c, W: Write + 'c, R: Read + 'c> Input<'c, W, R>
	{
		pub fn dispatch_key_event(&mut self, id: usize, etype: super::input::KeyEvent, text: Option<&str>) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: usize, params: Params<'s> }
			#[derive(Serialize)] struct Params<'s> { #[serde(rename = "type")] etype: super::input::KeyEvent, text: Option<&'s str> }
			self.0.send(&Payload { method: "Input.dispatchKeyEvent", id, params: Params { etype, text } })
		}

		pub fn dispatch_key_event_sync(&mut self, id: usize, etype: super::input::KeyEvent, text: Option<&str>) -> super::GenericResult<()>
		{
			self.dispatch_key_event(id, etype, text).map_err(From::from).and_then(|_| self.0.wait_result(id)).map(|_| ())
		}
	}
	pub struct Page<'c, W: Write + 'c, R: Read + 'c>(pub &'c mut Session<W, R>);
	impl<'c, W: Write + 'c, R: Read + 'c> Page<'c, W, R>
	{
		pub fn enable(&mut self, id: usize) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload { method: &'static str, id: usize }
			self.0.send(&Payload { method: "Page.enable", id })
		}
		pub fn navigate(&mut self, id: usize, url: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: usize, params: Params<'s> }
			#[derive(Serialize)] struct Params<'s> { url: &'s str }
			self.0.send(&Payload { method: "Page.navigate", id, params: Params { url } })
		}
		pub fn get_resource_tree(&mut self, id: usize) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload { method: &'static str, id: usize }
			self.0.send(&Payload { method: "Page.getResourceTree", id })
		}
		/// Experimental(stable版Chromeだと返り値がない)
		#[allow(dead_code)]
		pub fn create_isolated_world(&mut self, id: usize, frame_id: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: usize, params: Params<'s> }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { frame_id: &'s str }
			self.0.send(&Payload { method: "Page.createIsolatedWorld", id, params: Params { frame_id } })
		}

		pub fn navigate_sync(&mut self, id: usize, url: &str) -> super::GenericResult<()>
		{
			self.navigate(id, url).map_err(From::from).and_then(|_| self.0.wait_result(id)).map(|_| ())
		}
		pub fn get_resource_tree_sync(&mut self, id: usize) -> super::GenericResult<JValue>
		{
			self.get_resource_tree(id).map_err(From::from).and_then(|_| self.0.wait_result(id))
		}
		#[allow(dead_code)]
		pub fn create_isolated_world_sync(&mut self, id: usize, frame_id: &str) -> super::GenericResult<i64>
		{
			self.create_isolated_world(id, frame_id).map_err(From::from).and_then(|_| self.0.wait_result(id)).map(|v| v.as_i64().unwrap())
		}
	}
	pub struct Runtime<'c, W: Write + 'c, R: Read + 'c>(pub &'c mut Session<W, R>);
	impl<'c, W: Write + 'c, R: Read + 'c> Runtime<'c, W, R>
	{
		pub fn evaluate(&mut self, id: usize, expression: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: usize, params: Params<'s> }
			#[derive(Serialize)] struct Params<'s> { expression: &'s str }
			self.0.send(&Payload { method: "Runtime.evaluate", id, params: Params { expression } })
		}
		#[allow(dead_code)]
		pub fn evaluate_in(&mut self, id: usize, context_id: u64, expression: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: usize, params: Params<'s> }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { expression: &'s str, context_id: u64 }
			self.0.send(&Payload { method: "Runtime.evaluate", id, params: Params { expression, context_id } })
		}
		pub fn evaluate_value(&mut self, id: usize, expression: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: usize, params: Params<'s> }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { expression: &'s str, return_by_value: bool }
			self.0.send(&Payload { method: "Runtime.evaluate", id, params: Params { expression, return_by_value: true } })
		}
		pub fn evaluate_value_in(&mut self, id: usize, context_id: u64, expression: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: usize, params: Params<'s> }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { expression: &'s str, return_by_value: bool, context_id: u64 }
			self.0.send(&Payload { method: "Runtime.evaluate", id, params: Params { expression, return_by_value: true, context_id } })
		}
		pub fn get_properties(&mut self, id: usize, object_id: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: usize, params: Params<'s> }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { object_id: &'s str }
			self.0.send(&Payload { method: "Runtime.getProperties", id, params: Params { object_id } })
		}

		pub fn evaluate_sync(&mut self, id: usize, expression: &str) -> super::GenericResult<JValue>
		{
			self.evaluate(id, expression).map_err(From::from).and_then(|_| self.0.wait_result(id))
		}
		#[allow(dead_code)]
		pub fn evaluate_in_sync(&mut self, id: usize, context_id: u64, expression: &str) -> super::GenericResult<JValue>
		{
			self.evaluate_in(id, context_id, expression).map_err(From::from).and_then(|_| self.0.wait_result(id))
		}
		pub fn evaluate_value_sync(&mut self, id: usize, expression: &str) -> super::GenericResult<JValue>
		{
			self.evaluate_value(id, expression).map_err(From::from).and_then(|_| self.0.wait_result(id))
		}
		pub fn evaluate_value_in_sync(&mut self, id: usize, context_id: u64, expression: &str) -> super::GenericResult<JValue>
		{
			self.evaluate_value_in(id, context_id, expression).map_err(From::from).and_then(|_| self.0.wait_result(id))
		}
		pub fn get_properties_sync(&mut self, id: usize, object_id: &str) -> super::GenericResult<JValue>
		{
			self.get_properties(id, object_id).map_err(From::from).and_then(|_| self.0.wait_result(id))
		}
	}

	/// Event Handlable
	impl<'c, W: Write + 'c, R: Read + 'c> Runtime<'c, W, R>
	{
		/// Enables reporting of execution contexts creation by means of `executionContextCreated` event.
		/// When the reporting gets enabled the event will be sent immediately for each existing execution context.
		pub fn enable(&mut self, id: usize) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload { method: &'static str, id: usize }
			self.0.send(&Payload { method: "Runtime.enable", id })
		}
		/// Disables reporting of execution contexts creation
		pub fn disable(&mut self, id: usize) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload { method: &'static str, id: usize }
			self.0.send(&Payload { method: "Runtime.disable", id })
		}
	}
}
pub struct Process { process: Child, port: u16 }
impl Process
{
	pub fn run(port: u16, initial_url: &str) -> IOResult<Self>
	{
		#[cfg(windows)] const CHROME_DEFAULT_BIN: &'static str = r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe";
		#[cfg(unix)]    const CHROME_DEFAULT_BIN: &'static str = "google-chrome-stable";

		let chrome_bin = ::std::env::var("CHROME_BIN").unwrap_or_else(|_|
		{
			println!("Warning: $CHROME_BIN is not set, defaulting to \"{}\"", CHROME_DEFAULT_BIN);
			CHROME_DEFAULT_BIN.into()
		});
		let mut cmd = Command::new(&chrome_bin);
		cmd.args(&["--headless", "--disable-gpu", &format!("--remote-debugging-port={}", port), initial_url]);
		println!("[Headless Chrome]Launching {:?}...", cmd);
		let process = cmd.spawn()?;
		Self::wait_port_open(port)?;
		Ok(Process { process, port })
	}
	fn wait_port_open(port: u16) -> IOResult<()>
	{
		use std::time::Duration; use std::thread::sleep;
		use std::net::Shutdown;

		loop
		{
			match TcpStream::connect(format!("127.0.0.1:{}", port))
			{
				Ok(c) => { c.shutdown(Shutdown::Both).unwrap(); return Ok(()); },
				Err(e) => if e.kind() != IOErrorKind::ConnectionRefused { return Err(e); },
			}
			sleep(Duration::from_millis(100));
		}
	}
	pub fn get_sessions_async<C: Connect>(&self, client: &Client<C>) -> FutureResponse
	{
		client.get(format!("http://localhost:{}/json", self.port).parse().expect("Failed to parse URL"))
	}
	pub fn get_version_async<C: Connect>(&self, client: &Client<C>) -> FutureResponse
	{
		client.get(format!("http://localhost:{}/json/version", self.port).parse().expect("Failed to parse URL"))
	}
}
impl Drop for Process
{
	fn drop(&mut self) { self.process.kill().expect("Failed to kill the Headless Chrome"); }
}