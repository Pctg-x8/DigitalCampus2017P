
extern crate tokio_core;
extern crate hyper;
extern crate futures;
extern crate json_flex;
extern crate websocket;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate regex;

use tokio_core::reactor::Core;
use futures::{Future, Stream};
use std::io::prelude::*;
use regex::*;

/// An interface to the Headless Chrome
mod headless_chrome
{
	use serde::Serialize;
	use hyper::client::{Client, Connect, FutureResponse};
	use websocket::message::OwnedMessage;
	use websocket::WebSocketResult;
	use websocket::sender::Writer as WebSocketWriter;
	use websocket::receiver::Reader as WebSocketReader;
	use websocket::client::ClientBuilder;
	use std::process::{Child, Command};
	use std::io::prelude::{Write, Read};
	use std::net::TcpStream;
	use std::error::Error;
	use std::io::{Result as IOResult, ErrorKind as IOErrorKind};
	use serde_json::Value as JValue; use serde_json;

	type GenericResult<T> = Result<T, Box<Error>>;

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

	pub struct Session<W: Write, R: Read> { sender: WebSocketWriter<W>, receiver: WebSocketReader<R> }
	impl Session<TcpStream, TcpStream>
	{
		pub fn connect(addr: &str) -> Result<Self, Box<Error>>
		{
			let ws_client = ClientBuilder::new(addr)?.connect_insecure()?;
			let (recv, send) = ws_client.split()?;
			Ok(Session { sender: send, receiver: recv })
		}
	}
	impl<W: Write, R: Read> Session<W, R>
	{
		pub fn dom(&mut self) -> domain::DOM<W, R> { domain::DOM(self) }
		pub fn input(&mut self) -> domain::Input<W, R> { domain::Input(self) }
		pub fn page(&mut self) -> domain::Page<W, R> { domain::Page(self) }
		pub fn runtime(&mut self) -> domain::Runtime<W, R> { domain::Runtime(self) }
		pub fn wait_message(&mut self) -> WebSocketResult<OwnedMessage>
		{
			self.receiver.recv_message::<DummyIterator>()
		}
		pub fn wait_event<E: Event>(&mut self) -> GenericResult<E>
		{
			loop
			{
				match self.wait_message()?
				{
					OwnedMessage::Text(s) =>
					{
						// println!("[wait_event]Received: {}", s);
						// let obj: HashMap<_, _> = ::json_flex::decode(s).unwrap();
						let parsed: JValue = serde_json::from_str(&s).unwrap();
						if let Some(mtd) = parsed.get("method").and_then(JValue::as_str)
						{
							if mtd == "Page.frameNavigated"
							{
								println!("** Navigation Request: {}", parsed["params"]["frame"]["url"].as_str().unwrap());
							}
							if mtd == E::METHOD_NAME
							{
								return Ok(E::deserialize(&parsed.get("params").unwrap()));
							}
						}
						else if let Some(e) = parsed.get("error")
						{
							return Err(From::from(format!("RPC Error({}): {} in processing id {}", e["code"].as_i64().unwrap(),
								e["message"].as_str().unwrap(), parsed["id"].as_u64().unwrap())));
						}
					},
					_ => ()
				}
			}
		}
		pub fn wait_result(&mut self, id: usize) -> GenericResult<::serde_json::Value>
		{
			loop
			{
				match self.wait_message()?
				{
					OwnedMessage::Text(s) =>
					{
						// println!("[wait_result]Received: {}", s);
						// let mut obj: HashMap<_, _> = ::json_flex::decode(s).unwrap();
						let mut parser: ::serde_json::Value = ::serde_json::from_str(&s).unwrap();
						let obj = parser.as_object_mut().unwrap();
						if obj.contains_key("result")
						{
							if obj["id"].as_u64() == Some(id as u64) { return Ok(obj.remove("result").unwrap()); }
						}
						else if let Some(mtd) = obj.get("method").and_then(JValue::as_str)
						{
							if mtd == "Page.frameNavigated"
							{
								println!("** Navigation Request: {}", obj["params"]["frame"]["url"].as_str().unwrap());
							}
						}
						else if let Some(e) = obj.get("error")
						{
							return Err(From::from(format!("RPC Error({}): {} in processing id {}", e["code"].as_i64().unwrap(),
								e["message"].as_str().unwrap(), obj["id"].as_u64().unwrap())));
						}
					},
					_ => ()
				}
			}
		}
		fn send_text(&mut self, text: String) -> WebSocketResult<()>
		{
			// println!("Sending {}", text);
			self.sender.send_message(&OwnedMessage::Text(text))
		}
		fn send<T: Serialize>(&mut self, payload: &T) -> WebSocketResult<()>
		{
			self.send_text(::serde_json::to_string(payload).unwrap())
		}
	}
	pub trait Event: Sized
	{
		const METHOD_NAME: &'static str;
		fn deserialize(res: &JValue) -> Self;
	}
	pub mod dom
	{
		use std::io::prelude::*;
		use serde_json::Value as JValue;

		pub struct DocumentUpdated;
		impl super::Event for DocumentUpdated
		{
			const METHOD_NAME: &'static str = "DOM.documentUpdated";
			fn deserialize(_: &JValue) -> Self { DocumentUpdated }
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
			pub fn attributes(&mut self) -> super::GenericResult<Vec<::serde_json::Value>>
			{
				self.domain.get_attributes_sync(1000, self.id).map(|v| match v
				{
					::serde_json::Value::Object(mut o) => match o.remove("attributes")
					{
						Some(::serde_json::Value::Array(v)) => v,
						_ => panic!("Unexpected value type returned")
					},
					_ => panic!("Unexpected value type returned")
				})
			}
		}
	}
	#[allow(dead_code)]
	pub mod page
	{
		use serde_json::Value as JValue;

		pub struct LoadEventFired { timestamp: f64 }
		impl super::Event for LoadEventFired
		{
			const METHOD_NAME: &'static str = "Page.loadEventFired";
			fn deserialize(res: &JValue) -> Self
			{
				LoadEventFired { timestamp: res["timestamp"].as_f64().unwrap() }
			}
		}
		pub struct FrameStoppedLoading { pub frame_id: String }
		impl super::Event for FrameStoppedLoading
		{
			const METHOD_NAME: &'static str = "Page.frameStoppedLoading";
			fn deserialize(res: &JValue) -> Self
			{
				FrameStoppedLoading { frame_id: res["frameId"].as_str().unwrap().to_owned() }
			}
		}
		pub struct FrameNavigated { pub frame_id: String, pub name: Option<String>, pub url: String }
		impl super::Event for FrameNavigated
		{
			const METHOD_NAME: &'static str = "Page.frameNavigated";
			fn deserialize(res: &JValue) -> Self
			{
				FrameNavigated
				{
					frame_id: res["frame"]["id"].as_str().unwrap().to_owned(),
					name: res["frame"]["name"].as_str().map(|x| x.to_owned()),
					url: res["frame"]["url"].as_str().unwrap().to_owned()
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
	pub mod domain
	{
		use super::Session;
		use std::io::prelude::*;
		use websocket::WebSocketResult;

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

			pub fn get_document_sync(&mut self, id: usize) -> super::GenericResult<::serde_json::Value>
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
			pub fn query_selector_all_sync(&mut self, id: usize, node_id: isize, selector: &str) -> super::GenericResult<Vec<::serde_json::Value>>
			{
				self.query_selector_all(id, node_id, selector).map_err(From::from).and_then(|_| self.0.wait_result(id)).map(|o| match o
				{
					::serde_json::Value::Object(mut o) => match o.remove("nodeIds")
					{
						Some(::serde_json::Value::Array(v)) => v,
						_ => panic!("Unexpected value type returned")
					},
					_ => panic!("Unexpected value type returned")
				})
			}
			pub fn focus_sync(&mut self, id: usize, node_id: isize) -> super::GenericResult<()>
			{
				self.focus(id, node_id).map_err(From::from).and_then(|_| self.0.wait_result(id)).map(|_| ())
			}
			pub fn get_attributes_sync(&mut self, id: usize, node_id: isize) -> super::GenericResult<::serde_json::Value>
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
			pub fn get_resource_tree_sync(&mut self, id: usize) -> super::GenericResult<::serde_json::Value>
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
			pub fn evaluate_in(&mut self, id: usize, context_id: i64, expression: &str) -> WebSocketResult<()>
			{
				#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: usize, params: Params<'s> }
				#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { expression: &'s str, context_id: i64 }
				self.0.send(&Payload { method: "Runtime.evaluate", id, params: Params { expression, context_id } })
			}
			pub fn evaluate_value(&mut self, id: usize, expression: &str) -> WebSocketResult<()>
			{
				#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: usize, params: Params<'s> }
				#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { expression: &'s str, return_by_value: bool }
				self.0.send(&Payload { method: "Runtime.evaluate", id, params: Params { expression, return_by_value: true } })
			}
			pub fn get_properties(&mut self, id: usize, object_id: &str) -> WebSocketResult<()>
			{
				#[derive(Serialize)] struct Payload<'s> { method: &'static str, id: usize, params: Params<'s> }
				#[derive(Serialize)] #[serde(rename_all = "camelCase")] struct Params<'s> { object_id: &'s str }
				self.0.send(&Payload { method: "Runtime.getProperties", id, params: Params { object_id } })
			}

			pub fn evaluate_sync(&mut self, id: usize, expression: &str) -> super::GenericResult<::serde_json::Value>
			{
				self.evaluate(id, expression).map_err(From::from).and_then(|_| self.0.wait_result(id))
			}
			#[allow(dead_code)]
			pub fn evaluate_in_sync(&mut self, id: usize, context_id: i64, expression: &str) -> super::GenericResult<::serde_json::Value>
			{
				self.evaluate_in(id, context_id, expression).map_err(From::from).and_then(|_| self.0.wait_result(id))
			}
			pub fn evaluate_value_sync(&mut self, id: usize, expression: &str) -> super::GenericResult<::serde_json::Value>
			{
				self.evaluate_value(id, expression).map_err(From::from).and_then(|_| self.0.wait_result(id))
			}
			pub fn get_properties_sync(&mut self, id: usize, object_id: &str) -> super::GenericResult<::serde_json::Value>
			{
				self.get_properties(id, object_id).map_err(From::from).and_then(|_| self.0.wait_result(id))
			}
		}
	}
	pub struct Process { process: Child, port: u16 }
	impl Process
	{
		pub fn run(port: u16, initial_url: &str) -> IOResult<Self>
		{
			#[cfg(windows)] const CHROME_DEFAULT_BIN: &'static str = r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe";
			#[cfg(linux)]   const CHROME_DEFAULT_BIN: &'static str = "google-chrome-stable";

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
}

fn main()
{
	println!("DigitalCampus 2017 Prototype");

	let chrome = headless_chrome::Process::run(9222, "https://dh.force.com/digitalCampus/campusHomepage")
		.expect("Failed to launch the Headless Chrome");

	let mut tcore = Core::new().expect("Failed to initialize tokio-core");
	let client = hyper::Client::new(&tcore.handle());
	{
		let received = String::from_utf8_lossy(&tcore.run(chrome.get_version_async(&client).and_then(|res| res.body().concat2())).unwrap()).into_owned();
		let version_info: headless_chrome::BrowserVersion = serde_json::from_str(&received).unwrap();
		println!("Headless Chrome: {} :: {}", version_info.browser, version_info.protocol_version);
		println!("  webkit: {}", version_info.webkit_version);
		println!("  user-agent: {}", version_info.user_agent);
	}
	let session_list = 
	{
		let buffer = tcore.run(chrome.get_sessions_async(&client).and_then(|res| res.body().concat2())).unwrap();
		let list_js = json_flex::decode(String::from_utf8_lossy(&buffer).into_owned());
		list_js.into_vec().expect("Expeting Array").into_iter().map(|x| x["webSocketDebuggerUrl"].into_string().unwrap().clone()).collect::<Vec<_>>()
	};
	println!("Session URLs: {:?}", session_list);

	println!("Connecting {}...", session_list[0]);
	{
		let mut session = headless_chrome::Session::connect(&session_list[0]).expect("Failed to connect to a session in the Headless Chrome");
		session.page().enable(0).unwrap(); session.wait_result(0).unwrap();
		session.dom().enable(0).unwrap(); session.wait_result(0).unwrap();
		session.wait_event::<headless_chrome::page::LoadEventFired>().unwrap();
		let result_location = session.runtime().evaluate_sync(2, "location.href").unwrap();
		// let result = session.runtime().evaluate_sync(2, "document.querySelector('title').textContent").unwrap();
		let mut page_location = result_location["result"]["value"].as_str().unwrap().to_owned();
		/*let page_title = Regex::new(r"\\u([0-9a-fA-F]{4})").unwrap().replace_all(result["result"]["value"].as_str().unwrap(), |cap: &Captures|
		{
			String::from_utf16(&[u16::from_str_radix(&cap[1], 16).unwrap()]).unwrap()
		});*/
		// println!("Location: {}", page_location); println!("Page Title: {}", page_title);
		if page_location.contains("campuslogin")
		{
			// println!("Logging-in required for DigitalCampus");
			println!("デジキャンへのログインが必要です。");
		}
		while page_location.contains("campuslogin")
		{
			// Logging-in required
			// let id = prompt("Student Number");
			let id = prompt("学籍番号");
			disable_echo(); let pass = prompt(/*"Password"*/"パスワード"); enable_echo(); println!();
			// println!("Logging in as {}...", id.trim_right());
			println!("ログイン処理中です({})...", id.trim_right());
			// println!("Requesting {} {}", id.trim_right(), pass.trim_right());
			session.runtime().evaluate_sync(3, r#"document.querySelector('input[name="loginPage:formId:j_id33"]').value = "";"#).unwrap();
			session.dom().get_root_node_sync(4).unwrap().query_selector(r#"input[name="loginPage:formId:j_id33"]"#).unwrap().focus().unwrap();
			for c in id.trim_right().chars()
			{
				session.input().dispatch_key_event_sync(5, headless_chrome::input::KeyEvent::Char, Some(&c.to_string())).unwrap();
			}
			session.dom().get_root_node_sync(4).unwrap().query_selector(r#"input[name="loginPage:formId:j_id34"]"#).unwrap().focus().unwrap();
			for c in pass.trim_right().chars()
			{
				session.input().dispatch_key_event_sync(6, headless_chrome::input::KeyEvent::Char, Some(&c.to_string())).unwrap();
			}
			// press enter to login
			session.input().dispatch_key_event_sync(6, headless_chrome::input::KeyEvent::Char, Some("\r")).unwrap();
			session.wait_event::<headless_chrome::page::LoadEventFired>().unwrap();
			page_location = session.runtime().evaluate_sync(2, "location.href").unwrap()["result"]["value"].as_str().unwrap().to_owned();
			if page_location.contains("campuslogin")
			{
				// println!("** Failed to login to DigitalCampus. Check whether Student Number or password is correct **");
				println!("** デジキャンへのログインに失敗しました。学籍番号またはパスワードが正しいか確認してください。 **");
			}
		}
		while !page_location.contains("/campusHomepage")
		{
			session.wait_event::<headless_chrome::page::LoadEventFired>().unwrap();
			page_location = session.runtime().evaluate_sync(2, "location.href").unwrap()["result"]["value"].as_str().unwrap().to_owned();
		}
		println!("履修ページへアクセスしています...");
		// println!("Navigated: {}", page_location);
		// "履修・成績・出席"リンクを処理
		// 将来的にmenuBlockクラスが複数出てきたらまた考えます
		let intersys_link_path = "#gnav ul li.menuBlock ul li:first-child a";
		let intersys_link_attrs = session.dom().get_root_node_sync(8).unwrap().query_selector(intersys_link_path).unwrap().attributes().unwrap();
		let href_index = intersys_link_attrs.iter().enumerate().find(|&(_, s)| s == "href").map(|(i, _)| i + 1).unwrap();
		session.page().navigate_sync(9, intersys_link_attrs[href_index].as_str().unwrap()).unwrap();
		session.wait_event::<headless_chrome::page::LoadEventFired>().unwrap();
		// session.wait_event::<headless_chrome::dom::DocumentUpdated>().unwrap();

		// CampusPlanのほうは昔なつかしframesetで構成されているのでほしいフレームの中身に移動する
		let restree = session.page().get_resource_tree_sync(11).unwrap();
		let main_frame = restree["frameTree"]["childFrames"].as_array().unwrap().iter().find(|e| e["frame"]["name"] == "MainFrame").unwrap();
		session.page().navigate_sync(12, main_frame["frame"]["url"].as_str().unwrap()).unwrap();
		session.wait_event::<headless_chrome::page::LoadEventFired>().unwrap();
		session.runtime().evaluate(16, r#"document.querySelector('a#dgSystem__ctl2_lbtnSystemName').click()"#).unwrap();
		/*let course_link_path = "a#dgSystem__ctl2_lbtnSystemName";
		let course_link_attrs = session.dom().get_root_node_sync(13).unwrap().query_selector(course_link_path).unwrap().attributes().unwrap();
		// session.input().dispatch_key_event_sync(6, headless_chrome::input::KeyEvent::Char, Some("\r")).unwrap();
		let href_index = course_link_attrs.iter().enumerate().find(|&(_, s)| s == "href").map(|(i, _)| i + 1).unwrap();
		session.page().navigate_sync(14, course_link_attrs[href_index].as_str().unwrap()).unwrap();*/
		// onloadでコンテンツが読み込まれるので先に待つ
		session.wait_event::<headless_chrome::page::LoadEventFired>().unwrap();
		// 特定のフレームのロードを横取りする -> ほしいフレームだけ表示して操作
		let mut frame_nav_begin = session.wait_event::<headless_chrome::page::FrameNavigated>().unwrap();
		loop
		{
			if frame_nav_begin.name.as_ref().map(|x| x == "MainFrame").unwrap_or(false) { break; }
			frame_nav_begin = session.wait_event::<headless_chrome::page::FrameNavigated>().unwrap();
		}
		session.page().navigate_sync(15, &frame_nav_begin.url).unwrap();
		session.wait_event::<headless_chrome::page::LoadEventFired>().unwrap();
		session.runtime().evaluate(16, r#"document.querySelector('#dgSystem__ctl2_lbtnPage').click()"#).unwrap();
		session.wait_event::<headless_chrome::page::LoadEventFired>().unwrap();

		// ここまでで履修チェックページのデータは全部取れるはず

		// 学生プロファイル
		// セルで罫線を表現するというわけのわからない仕組みのため偶数行だけ取るようにしてる
		// 奇数列は項目の名前("学籍番号"とか)
		let profile_rows_data = session.runtime().evaluate_value_sync(20,
			r#"Array.prototype.map.call(document.querySelectorAll('#TableProfile tr:nth-child(2n) td:nth-child(2n)'), function(x){ return x.textContent; })"#)
			.unwrap();
		let regex_replace_encoded = Regex::new(r"\\u\{([0-9a-fA-F]{4})\}").unwrap();
		let profile_rows: Vec<_> = match profile_rows_data
		{
			serde_json::Value::Object(mut pro) => match pro.remove("result").unwrap()
			{
				serde_json::Value::Object(mut ro) => match ro.remove("value").expect("Unexpected value type returned")
				{
					serde_json::Value::Array(va) => va.into_iter().map(|v| match v
					{
						serde_json::Value::String(s) => regex_replace_encoded.replace_all(s.trim(), |cap: &Captures|
						{
							String::from_utf16(&[u16::from_str_radix(&cap[1], 16).unwrap()]).unwrap()
						}).into_owned(),
						_ => panic!("Unexpected value type returned")
					}).collect(),
					_ => panic!("Unexpected value type returned")
				},
				_ => panic!("Unexpected value type returned")
			},
			_ => panic!("Unexpected value type returned")
		};

		println!("=== 学生プロファイル ===");
		println!("** 学籍番号: {}", profile_rows[0]);
		println!("** 氏名: {}", profile_rows[1]);
		println!("** 学部/学年: {} {}", profile_rows[2], profile_rows[3]);
		println!("** セメスタ: {}", profile_rows[4]);
		println!("** 住所: {} {} {} {}", profile_rows[5], profile_rows[6], profile_rows[7], profile_rows[8]);

		// 履修テーブル(前半クォーター分だけ)の取得(クラス名の段階でわかるけどこれで3Q4Qどっちも取れる)
		// †履修テーブルの仕組み†
		// - 科目名が入るところは全部rishu-tbl-cellクラスっぽい(科目が入ってるところはbackground-colorスタイルが指定されて白くなっている)
		// - 科目があるセルはなんと3重table構造(はじめて見た)
		//   - 外側のtableは周囲に1pxの空きをつくるためのもの？
		//   - 2番目のtableが実際のコンテンツレイアウト
		//   - 3番目のtableは科目の詳細(2番目のtableにまとめられそうだけど)
		//   - ちなみに2番目の科目名と3番目は別の行に見えて同一のtd(tr)内(なぜ)
		//   - 空のセルにも1番目のtableだけ入ってる(自動生成の都合っぽい感じ)
		//     - これのおかげで若干空きセルに立体感が出る（？

		// 下のスクリプトで得られるデータは行優先です(0~5が1限、6~11が2限といった感じ)
		let course_table = match session.runtime().evaluate_value_sync(21, r#"
			var tables = document.querySelectorAll('table.rishu-tbl-cell');
			// 前半クォーターは3、後半クォーターは5
			var q1_koma_cells = tables[3].querySelectorAll('td.rishu-tbl-cell');
			var q2_koma_cells = tables[5].querySelectorAll('td.rishu-tbl-cell');
			[Array.prototype.map.call(q1_koma_cells, function(k)
			{
				var title_link = k.querySelector('a');
				if(!title_link) return null; else return title_link.textContent;
			}), Array.prototype.map.call(q2_koma_cells, function(k)
			{
				var title_link = k.querySelector('a');
				if(!title_link) return null; else return title_link.textContent;
			})]
		"#).unwrap()
		{
			serde_json::Value::Object(mut ro) => match ro.remove("result")
			{
				Some(serde_json::Value::Object(mut vo)) => match vo.remove("value")
				{
					Some(serde_json::Value::Array(v)) => v.into_iter().map(|v| match v
					{
						serde_json::Value::Array(vi) => vi.into_iter().map(|vs| match vs
						{
							serde_json::Value::Null => String::new(),
							serde_json::Value::String(s) => s,
							_ => panic!("Unexpected value type returned")
						}).collect(),
						_ => panic!("Unexpected value type returned")
					}).collect::<Vec<Vec<_>>>(),
					_ => panic!("Unexpected value type returned")
				},
				_ => panic!("Unexpected value type returned")
			},
			_ => panic!("Unexpected value type returned")
		};
		println!("CourseTable FirstQuarter: {:?}", course_table[0]);
		println!("CourseTable LastQuarter: {:?}", course_table[1]);
		
		/*
		// vvv Experimental環境で有効(だとおもわれる) vvv
		// 別フレームの要素にアクセスできないらしいので新しくIsolatedなContextを作る
		let frame_context = session.page().create_isolated_world_sync(15, &frame_nav_begin.frame_id).unwrap();
		// 履修結果ページを開く(登録期間中はこれだと動かないかもしれない)
		session.runtime().evaluate_in(16, frame_context, r#"document.querySelector('#dgSystem__ctl2_lbtnPage').click()"#).unwrap();
		*/

		/*loop
		{
			match session.wait_message().unwrap()
			{
				websocket::message::OwnedMessage::Text(s) =>
				{
					println!("Receive: {}", s);
				},
				_ => ()
			}
			// std::io::stdin().read(&mut [0]).unwrap();
		}*/
	}
}

fn prompt(text: &str) -> String
{
	write!(std::io::stdout(), "{}>", text).unwrap(); std::io::stdout().flush().unwrap();
	let mut s = String::new();
	std::io::stdin().read_line(&mut s).unwrap(); s
}

// platform dependent - POSIX(Linux)
#[cfg(linux)]
extern crate termios;
#[cfg(linux)]
const STDIN_FD: std::os::unix::io::RawFd = 0;
#[cfg(linux)]
fn disable_echo()
{
	use termios::Termios;
	let mut tio = Termios::from_fd(STDIN_FD).unwrap();
	tio.c_lflag &= !termios::ECHO;
	termios::tcsetattr(STDIN_FD, termios::TCSANOW, &tio).unwrap();
}
#[cfg(linux)]
fn enable_echo()
{
	use termios::Termios;
	let mut tio = Termios::from_fd(STDIN_FD).unwrap();
	tio.c_lflag |= termios::ECHO;
	termios::tcsetattr(STDIN_FD, termios::TCSANOW, &tio).unwrap();
}

// platform dependent - Win32
#[cfg(windows)] extern crate winapi;
#[cfg(windows)] extern crate kernel32;
#[cfg(windows)] fn disable_echo()
{
	let hstdin = unsafe { kernel32::GetStdHandle(winapi::STD_INPUT_HANDLE) };
	let mut mode = 0;
	unsafe { kernel32::GetConsoleMode(hstdin, &mut mode) };
	unsafe { kernel32::SetConsoleMode(hstdin, mode & !winapi::ENABLE_ECHO_INPUT) };
}
#[cfg(windows)] fn enable_echo()
{
	let hstdin = unsafe { kernel32::GetStdHandle(winapi::STD_INPUT_HANDLE) };
	let mut mode = 0;
	unsafe { kernel32::GetConsoleMode(hstdin, &mut mode) };
	unsafe { kernel32::SetConsoleMode(hstdin, mode | winapi::ENABLE_ECHO_INPUT) };
}
