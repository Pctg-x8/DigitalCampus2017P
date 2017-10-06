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
use std::mem::transmute_copy;
use serde_json::{Value as JValue}; use serde_json;
use GenericResult;
use serde::de::DeserializeOwned;

// primitives
pub type RequestID = u64;

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

/// `json` response(session list)
#[derive(Deserialize)] #[serde(rename_all = "camelCase")]
pub struct SessionInfo<'s>
{
	pub description: &'s str, pub id: &'s str, pub title: &'s str,
	#[serde(rename = "type")]
	pub _type: &'s str, pub url: &'s str, pub web_socket_debugger_url: Option<&'s str>
}

struct DummyIterator;
impl Iterator for DummyIterator
{
	type Item = ::websocket::dataframe::DataFrame;
	fn next(&mut self) -> Option<Self::Item> { None }
}

/// Subscribes an event of `E`
pub trait FrameNavigatedEventSubscriber
{
	fn on_event(&mut self, event: &page::FrameNavigated);
}
impl<F> FrameNavigatedEventSubscriber for F where F: for<'r> FnMut(&'r page::FrameNavigated)
{
	fn on_event(&mut self, event: &page::FrameNavigated) { self(event) }
}
pub trait Event: Sized
{
	const METHOD_NAME: &'static str;
}

#[derive(Deserialize)] #[serde(untagged)]
pub enum SessionReceiveEvent<'s>
{
	Result { id: RequestID, result: JValue },
	Error { id: RequestID, error: ErrorDescription<'s> },
	Method { method: &'s str, params: JValue }
}
#[derive(Deserialize)]
pub struct ErrorDescription<'s> { code: i64, message: &'s str }
impl<'s> SessionReceiveEvent<'s>
{
	pub fn error_text(&self) -> Option<String>
	{
		if let &SessionReceiveEvent::Error { id, ref error } = self
		{
			Some(format!("RPC Error({}): {} in processing id {}", error.code, error.message, id))
		}
		else { None }
	}
}

pub struct Session<W: Write, R: Read>
{
	sender: WebSocketWriter<W>, receiver: WebSocketReader<R>,
	frame_navigated_event_subscriber: Vec<*mut FrameNavigatedEventSubscriber>
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
	pub fn network(&mut self) -> domain::Network<W, R> { domain::Network(self) }
	pub fn page(&mut self) -> domain::Page<W, R> { domain::Page(self) }
	pub fn runtime(&mut self) -> domain::Runtime<W, R> { domain::Runtime(self) }
}
impl<W: Write, R: Read> Session<W, R>
{
	pub fn subscribe_session_event(&mut self, subscriber: &'static FrameNavigatedEventSubscriber)
	{
		self.frame_navigated_event_subscriber.push(subscriber as *const _ as *mut _);
	}
	pub fn unsubscribe_session_event(&mut self, subscriber: &'static FrameNavigatedEventSubscriber)
	{
		if let Some(index) = self.frame_navigated_event_subscriber.iter().position(|&x| x == subscriber as *const _ as *mut _)
		{
			self.frame_navigated_event_subscriber.remove(index);
		}
	}
}
#[cfg(feature = "verbose")] use colored::*;
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
	pub fn dispatch_frame_navigated(&self, e: &page::FrameNavigated)
	{
		for &call in &self.frame_navigated_event_subscriber { unsafe { &mut *call }.on_event(e); }
	}
	pub fn wait_event<E: Event + DeserializeOwned>(&mut self) -> GenericResult<E>
	{
		loop
		{
			let s = self.wait_text()?;
			#[cfg(feature = "verbose")] println!("{}", format!("<<-- [wait_event]Received: {}", s).blue().bold());
			let obj: SessionReceiveEvent = serde_json::from_str(&s)?;
			match obj
			{
				e@SessionReceiveEvent::Error { .. } => return Err(e.error_text().unwrap().into()),
				SessionReceiveEvent::Method { method: name, params } =>
				{
					if name == page::FrameNavigated::METHOD_NAME
					{
						let e: page::FrameNavigatedOwned = serde_json::from_value(params)?;
						self.dispatch_frame_navigated(&e.borrow());
						if name == E::METHOD_NAME { return Ok(unsafe { transmute_copy(&e) }); }
					}
					else if name == E::METHOD_NAME { return serde_json::from_value(params).map_err(From::from); }
				}
				_ => ()
			}
		}
	}
	pub fn wait_result(&mut self, id: RequestID) -> GenericResult<JValue>
	{
		loop
		{
			let s = self.wait_text()?;
			#[cfg(feature = "verbose")] println!("{}", format!("<<-- [wait_result]Received: {}", s).blue().bold());
			let obj: SessionReceiveEvent = serde_json::from_str(&s)?;
			match obj
			{
				e@SessionReceiveEvent::Error { .. } => return Err(e.error_text().unwrap().into()),
				SessionReceiveEvent::Method { method: name, params } =>
				{
					if name == page::FrameNavigated::METHOD_NAME
					{
						self.dispatch_frame_navigated(&serde_json::from_value::<page::FrameNavigatedOwned>(params)?.borrow());
					}
				},
				SessionReceiveEvent::Result { id: rid, result } => if rid == id { return Ok(result); },
			}
		}
	}
	fn send_text(&mut self, text: String) -> WebSocketResult<()>
	{
		// println!("Sending {}", text);
		#[cfg(feature = "verbose")] println!("{}", format!("-->> [send]Sending: {}", text).green().bold());
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
	use serde_json::{Value as JValue};

	#[derive(Deserialize)]
	pub struct DocumentUpdated;
	impl super::Event for DocumentUpdated
	{
		const METHOD_NAME: &'static str = "DOM.documentUpdated";
	}

	pub struct Node<'s, 'c: 's, W: Write + 'c, R: Read + 'c> { pub domain: &'s mut super::domain::DOM<'c, W, R>, pub id: isize }
	impl<'s, 'c: 's, W: Write + 'c, R: Read + 'c> Node<'s, 'c, W, R>
	{
		pub fn query_selector<'ss: 's>(&'ss mut self, selector: &str) -> super::GenericResult<Node<'s, 'c, W, R>>
		{
			self.domain.query_selector_sync(1000, self.id, selector).map(move |nid| Node { domain: self.domain, id: nid })
		}
		pub fn query_selector_nth<'ss: 's>(&'ss mut self, selector: &str, index: usize) -> super::GenericResult<Node<'s, 'c, W, R>>
		{
			let ref nid = self.domain.query_selector_all_sync(1000, self.id, selector)?[index];
			Ok(Node { domain: self.domain, id: nid.as_i64().unwrap() as _ })
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
	#[derive(Deserialize, Clone, Copy)] #[serde(rename_all = "camelCase")]
	pub struct LoadEventFired { timestamp: f64 }
	impl super::Event for LoadEventFired
	{
		const METHOD_NAME: &'static str = "Page.loadEventFired";
	}
	#[derive(Deserialize)] #[serde(rename_all = "camelCase")]
	pub struct FrameStoppedLoading<'d> { pub frame_id: &'d str }
	#[derive(Deserialize)] #[serde(rename_all = "camelCase")]
	pub struct FrameStoppedLoadingOwned { pub frame_id: String }
	impl<'d> super::Event for FrameStoppedLoading<'d>
	{
		const METHOD_NAME: &'static str = "Page.frameStoppedLoading";
	}
	impl super::Event for FrameStoppedLoadingOwned { const METHOD_NAME: &'static str = "Page.frameStoppedLoading"; }
	impl FrameStoppedLoadingOwned
	{
		pub fn borrow(&self) -> FrameStoppedLoading { FrameStoppedLoading { frame_id: &self.frame_id } }
	}
	impl<'d> FrameStoppedLoading<'d>
	{
		pub fn to_owned(&self) -> FrameStoppedLoadingOwned { FrameStoppedLoadingOwned { frame_id: self.frame_id.to_owned() } }
	}
	#[derive(Deserialize)] #[serde(rename_all = "camelCase")]
	pub struct FrameNavigated<'d> { #[serde(borrow = "'d")] pub frame: Frame<'d> }
	#[derive(Deserialize)] #[serde(rename_all = "camelCase")]
	pub struct FrameNavigatedOwned { pub frame: FrameOwned }
	impl<'d> super::Event for FrameNavigated<'d>  { const METHOD_NAME: &'static str = "Page.frameNavigated"; }
	impl     super::Event for FrameNavigatedOwned { const METHOD_NAME: &'static str = "Page.frameNavigated"; }
	impl FrameNavigatedOwned
	{
		pub fn borrow(&self) -> FrameNavigated { FrameNavigated { frame: self.frame.borrow() } }
	}
	impl<'d> FrameNavigated<'d>
	{
		pub fn to_owned(&self) -> FrameNavigatedOwned { FrameNavigatedOwned { frame: self.frame.to_owned() } }
	}

	#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)] #[serde(rename_all = "camelCase")]
	pub struct FrameOwned
	{
		pub id: String, pub parent_id: Option<String>, pub loader_id: String,
		pub name: Option<String>, pub url: String, pub security_origin: String,
		pub mime_type: String
	}
	#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)] #[serde(rename_all = "camelCase")]
	pub struct Frame<'d>
	{
		pub id: &'d str, pub parent_id: Option<&'d str>, pub loader_id: &'d str,
		pub name: Option<&'d str>, pub url: &'d str, pub security_origin: &'d str,
		pub mime_type: &'d str
	}
	impl<'d> Frame<'d>
	{
		pub fn to_owned(&self) -> FrameOwned
		{
			FrameOwned
			{
				id: self.id.to_owned(), parent_id: self.parent_id.as_ref().map(|&x| x.to_owned()),
				loader_id: self.loader_id.to_owned(), name: self.name.as_ref().map(|&x| x.to_owned()),
				url: self.url.to_owned(), security_origin: self.security_origin.to_owned(),
				mime_type: self.mime_type.to_owned()
			}
		}
	}
	impl FrameOwned
	{
		pub fn borrow(&self) -> Frame
		{
			Frame
			{
				id: &self.id, parent_id: self.parent_id.as_ref().map(|x| x as &str),
				loader_id: &self.loader_id, name: self.name.as_ref().map(|x| x as &str),
				url: &self.url, security_origin: &self.security_origin, mime_type: &self.mime_type
			}
		}
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

	#[derive(Deserialize, Clone)] #[serde(rename_all = "camelCase")]
	pub struct ExecutionContextCreated { pub context: ExecutionContextDescription }
	impl super::Event for ExecutionContextCreated
	{
		const METHOD_NAME: &'static str = "Runtime.executionContextCreated";
	}
	#[derive(Deserialize, Clone, Copy)] #[serde(rename_all = "camelCase")]
	pub struct ExecutionContextDestroyed { pub execution_context_id: ExecutionContextID }
	impl super::Event for ExecutionContextDestroyed
	{
		const METHOD_NAME: &'static str = "Runtime.executionContextDestroyed";
	}
	#[derive(Deserialize)]
	pub struct ExecutionContextsCleared {}
	impl super::Event for ExecutionContextsCleared
	{
		const METHOD_NAME: &'static str = "Runtime.executionContextsCleared";
	}

	/// Unique script identifier
	pub type ScriptID = String;
	/// Unique object identifier
	pub type RemoteObjectID = String;
	/// Primitive value which cannot be JSON^stringified
	pub type UnserializableValue = String;
	/// ID ob an execution context
	pub type ExecutionContextID = u64;
	/// Mirror object referencing original JavaScript object.
	#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)] #[serde(rename_all = "camelCase")]
	pub struct RemoteObject
	{
		#[serde(rename = "type")] pub type_: ObjectType,
		pub subtype: Option<ObjectSubtype>, pub class_name: Option<String>,
		pub value: Option<JValue>, pub unserializable_value: Option<UnserializableValue>,
		pub description: Option<String>, pub object_id: Option<RemoteObjectID>
	}
	/// Description of an isolated world
	#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)] #[serde(rename_all = "camelCase")]
	pub struct ExecutionContextDescription
	{
		/// Unique id of the execution context
		pub id: ExecutionContextID,
		/// Execution context origin
		pub origin: String,
		/// Human readable name describing given context
		pub name: String,
		/// Embedder-specific auxiliary data
		pub aux_data: Option<JMap<String, JValue>>
	}
	#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)] #[serde(rename_all = "camelCase")]
	pub struct ExceptionDetails
	{
		pub exception_id: u64, pub text: String, pub line_number: u64, pub column_number: u64,
		pub script_id: Option<ScriptID>, pub url: Option<String>, pub stack_trace: Option<StackTrace>,
		pub exception: Option<RemoteObject>, pub execution_context_id: Option<ExecutionContextID>
	}
	#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)] #[serde(rename_all = "camelCase")]
	pub struct StackTrace
	{
		pub description: Option<String>, pub call_frames: Vec<CallFrame>, pub parent: Option<Box<StackTrace>>
	}
	#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)] #[serde(rename_all = "camelCase")]
	pub struct CallFrame
	{
		pub function_name: String, pub script_id: ScriptID, pub url: String,
		pub line_number: u64, pub column_number: u64
	}

	#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)] #[serde(rename_all = "camelCase")]
	pub enum ObjectType { Object, Function, Undefined, String, Number, Boolean, Symbol }
	#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)] #[serde(rename_all = "camelCase")]
	pub enum ObjectSubtype
	{
		Array, Null, Node, Regexp, Date, Map, Set, Iterator, Generator, Error, Proxy, Promise, TypedArray
	}
	#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)] #[serde(rename_all = "camelCase")]
	pub struct EvaluateResult { pub result: RemoteObject, pub exception_details: Option<ExceptionDetails> }

	/// Typing Helpers
	impl RemoteObject
	{
		pub fn strip_value(self) -> JValue { self.value.expect("Illegal use of API or API Corruption") }
		pub fn assume_string(self) -> String
		{
			match self.value { Some(JValue::String(s)) => s, _ => api_corruption!(value_type) }
		}
		pub fn assume_object(self) -> JMap<String, JValue>
		{
			match self.value { Some(JValue::Object(s)) => s, _ => api_corruption!(value_type) }
		}
		pub fn assume_array(self) -> Vec<JValue>
		{
			match self.value { Some(JValue::Array(s)) => s, _ => api_corruption!(value_type) }
		}
	}
	pub trait JSONTyping<T>
	{
		fn assume(self) -> T;
	}
	impl JSONTyping<String> for RemoteObject { fn assume(self) -> String { self.assume_string() } }
	impl JSONTyping<Vec<JValue>> for RemoteObject { fn assume(self) -> Vec<JValue> { self.assume_array() } }
}
pub mod domain
{
	use super::{Session, RequestID};
	use std::io::prelude::*;
	use websocket::WebSocketResult;
	use serde_json::Value as JValue;

	pub struct DOM<'c, W: Write + 'c, R: Read + 'c>(pub &'c mut Session<W, R>);
	impl<'c, W: Write + 'c, R: Read + 'c> DOM<'c, W, R>
	{
		pub fn enable(&mut self, id: RequestID) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload { method: &'static str, id: RequestID }
			self.0.send(&Payload { method: "DOM.enable", id })
		}
		pub fn get_document(&mut self, id: RequestID) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload { method: &'static str, id: RequestID }
			self.0.send(&Payload { method: "DOM.getDocument", id })
		}
		pub fn query_selector(&mut self, id: RequestID, node_id: isize, selector: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: RequestID, params: Params<'s> }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { node_id: isize, selector: &'s str }
			self.0.send(&Payload { method: "DOM.querySelector", id, params: Params { node_id, selector } })
		}
		pub fn query_selector_all(&mut self, id: RequestID, node_id: isize, selector: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: RequestID, params: Params<'s> }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { node_id: isize, selector: &'s str }
			self.0.send(&Payload { method: "DOM.querySelectorAll", id, params: Params { node_id, selector } })
		}
		pub fn focus(&mut self, id: RequestID, node_id: isize) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload { method: &'static str, id: RequestID, params: Params }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params { node_id: isize }
			self.0.send(&Payload { method: "DOM.focus", id, params: Params { node_id } })
		}
		pub fn get_attributes(&mut self, id: RequestID, node_id: isize) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload { method: &'static str, id: RequestID, params: Params }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params { node_id: isize }
			self.0.send(&Payload { method: "DOM.getAttributes", id, params: Params { node_id } })
		}

		pub fn get_document_sync(&mut self, id: RequestID) -> super::GenericResult<JValue>
		{
			self.get_document(id).map_err(From::from).and_then(|_| self.0.wait_result(id))
		}
		pub fn get_root_node_sync<'s>(&'s mut self, id: RequestID) -> super::GenericResult<super::dom::Node<'s, 'c, W, R>>
		{
			self.get_document_sync(id).map(move |id| super::dom::Node { domain: self, id: id["root"]["nodeId"].as_i64().unwrap() as isize })
		}
		pub fn query_selector_sync(&mut self, id: RequestID, node_id: isize, selector: &str) -> super::GenericResult<isize>
		{
			self.query_selector(id, node_id, selector).map_err(From::from).and_then(|_| self.0.wait_result(id))
				.map(|o| o["nodeId"].as_i64().unwrap() as isize)
		}
		pub fn query_selector_all_sync(&mut self, id: RequestID, node_id: isize, selector: &str) -> super::GenericResult<Vec<JValue>>
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
		pub fn focus_sync(&mut self, id: RequestID, node_id: isize) -> super::GenericResult<()>
		{
			self.focus(id, node_id).map_err(From::from).and_then(|_| self.0.wait_result(id)).map(|_| ())
		}
		pub fn get_attributes_sync(&mut self, id: RequestID, node_id: isize) -> super::GenericResult<JValue>
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
		pub fn dispatch_key_event(&mut self, id: RequestID, etype: super::input::KeyEvent, text: Option<&str>) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: RequestID, params: Params<'s> }
			#[derive(Serialize)] struct Params<'s> { #[serde(rename = "type")] etype: super::input::KeyEvent, text: Option<&'s str> }
			self.0.send(&Payload { method: "Input.dispatchKeyEvent", id, params: Params { etype, text } })
		}

		pub fn dispatch_key_event_sync(&mut self, id: RequestID, etype: super::input::KeyEvent, text: Option<&str>) -> super::GenericResult<()>
		{
			self.dispatch_key_event(id, etype, text).map_err(From::from).and_then(|_| self.0.wait_result(id)).map(|_| ())
		}
	}
	pub struct Network<'c, W: Write + 'c, R: Read + 'c>(pub &'c mut Session<W, R>);
	impl<'c, W: Write + 'c, R: Read + 'c> Network<'c, W, R>
	{
		pub fn set_user_agent_override(&mut self, id: RequestID, ua: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: RequestID, params: Params<'s> }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { user_agent: &'s str }
			self.0.send(&Payload { method: "Network.setUserAgentOverride", id, params: Params { user_agent: ua } })
		}
	}
	pub struct Page<'c, W: Write + 'c, R: Read + 'c>(pub &'c mut Session<W, R>);
	impl<'c, W: Write + 'c, R: Read + 'c> Page<'c, W, R>
	{
		pub fn enable(&mut self, id: RequestID) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload { method: &'static str, id: RequestID }
			self.0.send(&Payload { method: "Page.enable", id })
		}
		pub fn navigate(&mut self, id: RequestID, url: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: RequestID, params: Params<'s> }
			#[derive(Serialize)] struct Params<'s> { url: &'s str }
			self.0.send(&Payload { method: "Page.navigate", id, params: Params { url } })
		}
		pub fn get_resource_tree(&mut self, id: RequestID) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload { method: &'static str, id: RequestID }
			self.0.send(&Payload { method: "Page.getResourceTree", id })
		}
		/// Experimental(stable版Chromeだと返り値がない)
		#[allow(dead_code)]
		pub fn create_isolated_world(&mut self, id: RequestID, frame_id: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: RequestID, params: Params<'s> }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { frame_id: &'s str }
			self.0.send(&Payload { method: "Page.createIsolatedWorld", id, params: Params { frame_id } })
		}

		pub fn navigate_sync(&mut self, id: RequestID, url: &str) -> super::GenericResult<()>
		{
			self.navigate(id, url).map_err(From::from).and_then(|_| self.0.wait_result(id)).map(|_| ())
		}
		pub fn get_resource_tree_sync(&mut self, id: RequestID) -> super::GenericResult<JValue>
		{
			self.get_resource_tree(id).map_err(From::from).and_then(|_| self.0.wait_result(id))
		}
		#[allow(dead_code)]
		pub fn create_isolated_world_sync(&mut self, id: RequestID, frame_id: &str) -> super::GenericResult<i64>
		{
			self.create_isolated_world(id, frame_id).map_err(From::from).and_then(|_| self.0.wait_result(id)).map(|v| v.as_i64().unwrap())
		}
	}
	pub struct Runtime<'c, W: Write + 'c, R: Read + 'c>(pub &'c mut Session<W, R>);
	impl<'c, W: Write + 'c, R: Read + 'c> Runtime<'c, W, R>
	{
		pub fn evaluate(&mut self, id: RequestID, expression: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: RequestID, params: Params<'s> }
			#[derive(Serialize)] struct Params<'s> { expression: &'s str }
			self.0.send(&Payload { method: "Runtime.evaluate", id, params: Params { expression } })
		}
		#[allow(dead_code)]
		pub fn evaluate_in(&mut self, id: RequestID, context_id: u64, expression: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: RequestID, params: Params<'s> }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { expression: &'s str, context_id: u64 }
			self.0.send(&Payload { method: "Runtime.evaluate", id, params: Params { expression, context_id } })
		}
		pub fn evaluate_value(&mut self, id: RequestID, expression: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: RequestID, params: Params<'s> }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { expression: &'s str, return_by_value: bool }
			self.0.send(&Payload { method: "Runtime.evaluate", id, params: Params { expression, return_by_value: true } })
		}
		pub fn evaluate_value_in(&mut self, id: RequestID, context_id: u64, expression: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: RequestID, params: Params<'s> }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { expression: &'s str, return_by_value: bool, context_id: u64 }
			self.0.send(&Payload { method: "Runtime.evaluate", id, params: Params { expression, return_by_value: true, context_id } })
		}
		pub fn get_properties(&mut self, id: RequestID, object_id: &str) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: RequestID, params: Params<'s> }
			#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { object_id: &'s str }
			self.0.send(&Payload { method: "Runtime.getProperties", id, params: Params { object_id } })
		}

		pub fn evaluate_sync(&mut self, id: RequestID, expression: &str) -> super::GenericResult<super::runtime::EvaluateResult>
		{
			self.evaluate(id, expression).map_err(From::from).and_then(|_| self.0.wait_result(id))
				.and_then(|x| ::serde_json::from_value(x).map_err(From::from))
		}
		#[allow(dead_code)]
		pub fn evaluate_in_sync(&mut self, id: RequestID, context_id: u64, expression: &str) -> super::GenericResult<super::runtime::EvaluateResult>
		{
			self.evaluate_in(id, context_id, expression).map_err(From::from).and_then(|_| self.0.wait_result(id))
				.and_then(|x| ::serde_json::from_value(x).map_err(From::from))
		}
		pub fn evaluate_value_sync(&mut self, id: RequestID, expression: &str) -> super::GenericResult<super::runtime::EvaluateResult>
		{
			self.evaluate_value(id, expression).map_err(From::from).and_then(|_| self.0.wait_result(id))
				.and_then(|x| ::serde_json::from_value(x).map_err(From::from))
		}
		pub fn evaluate_value_in_sync(&mut self, id: RequestID, context_id: u64, expression: &str) -> super::GenericResult<super::runtime::EvaluateResult>
		{
			self.evaluate_value_in(id, context_id, expression).map_err(From::from).and_then(|_| self.0.wait_result(id))
				.and_then(|x| ::serde_json::from_value(x).map_err(From::from))
		}
		pub fn get_properties_sync(&mut self, id: RequestID, object_id: &str) -> super::GenericResult<JValue>
		{
			self.get_properties(id, object_id).map_err(From::from).and_then(|_| self.0.wait_result(id))
		}
	}

	/// Event Handlable
	impl<'c, W: Write + 'c, R: Read + 'c> Runtime<'c, W, R>
	{
		/// Enables reporting of execution contexts creation by means of `executionContextCreated` event.
		/// When the reporting gets enabled the event will be sent immediately for each existing execution context.
		pub fn enable(&mut self, id: RequestID) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload { method: &'static str, id: RequestID }
			self.0.send(&Payload { method: "Runtime.enable", id })
		}
		/// Disables reporting of execution contexts creation
		pub fn disable(&mut self, id: RequestID) -> WebSocketResult<()>
		{
			#[derive(Serialize)] struct Payload { method: &'static str, id: RequestID }
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