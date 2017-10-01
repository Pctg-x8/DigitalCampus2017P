
#![feature(iterator_step_by)]

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
use std::error::Error;

macro_rules! api_corruption
{
	(value_type) => (panic!("Unexpected value type returned. the API may be corrupted"))
}
macro_rules! jvDecomposite
{
	{ $v: expr => object[$inner: pat]: $e: expr } =>
	{
		match $v { JValue::Object($inner) => $e, _ => api_corruption!(value_type) }
	};
	{ $v: expr => array[$inner: pat]: $e: expr } =>
	{
		match $v { JValue::Array($inner) => $e, _ => api_corruption!(value_type) }
	};
	{ $v: expr => string[$inner: pat]: $e: expr } =>
	{
		match $v { JValue::String($inner) => $e, _ => api_corruption!(value_type) }
	};
	{ $v: expr => opt object[$inner: pat]: $e: expr } =>
	{
		match $v { Some(JValue::Object($inner)) => $e, _ => api_corruption!(value_type) }
	};
	{ $v: expr => opt array[$inner: pat]: $e: expr } =>
	{
		match $v { Some(JValue::Array($inner)) => $e, _ => api_corruption!(value_type) }
	};
	{ $v: expr => opt string[$inner: pat]: $e: expr } =>
	{
		match $v { Some(JValue::String($inner)) => $e, _ => api_corruption!(value_type) }
	};
}

type GenericResult<T> = Result<T, Box<Error>>;

mod headless_chrome;
mod remote_campus;
use remote_campus::RemoteCampus;

fn process_login(mut pctrl: remote_campus::LoginPage) -> remote_campus::HomePage
{
	// Logging-in required
	// let id = prompt("Student Number");
	let id = prompt("学籍番号");
	disable_echo(); let pass = prompt(/*"Password"*/"パスワード"); enable_echo(); println!();
	// println!("Logging in as {}...", id.trim_right());
	println!("ログイン処理中です({})...", id.trim());
	pctrl.set_login_info_fields(id.trim(), pass.trim()).unwrap();
	pctrl.submit().expect("Error logging in").unwrap_or_else(|e|
	{
		// println!("** Failed to login to DigitalCampus. Check whether Student Number or password is correct **");
		println!("** デジキャンへのログインに失敗しました。学籍番号またはパスワードが正しいか確認してください。 **");
		process_login(e)
	})
}

fn main()
{
	println!("DigitalCampus 2017 Prototype");

	let autologin = std::env::args().nth(1).map(|s| s.split(":").map(ToOwned::to_owned).collect::<Vec<String>>());

	let chrome = headless_chrome::Process::run(9222, "https://dh.force.com/digitalCampus/campusHomepage").expect("Failed to launch the Headless Chrome");

	let mut tcore = Core::new().expect("Failed to initialize tokio-core");
	let client = hyper::Client::new(&tcore.handle());
	{
		let received = String::from_utf8_lossy(&tcore.run(chrome.get_version_async(&client).and_then(|res| res.body().concat2())).unwrap()).into_owned();
		let version_info: headless_chrome::BrowserVersion = serde_json::from_str(&received).unwrap();
		println!("Headless Chrome: {} :: {}", version_info.browser, version_info.protocol_version);
		println!("  webkit: {}", version_info.webkit_version);
		println!("  user-agent: {}", version_info.user_agent);
	}
	let session_list: Vec<String> = 
	{
		let buffer = tcore.run(chrome.get_sessions_async(&client).and_then(|res| res.body().concat2())).unwrap();
		let list_js = json_flex::decode(String::from_utf8_lossy(&buffer).into_owned());
		list_js.into_vec().expect("Expeting Array").into_iter().map(|x| x["webSocketDebuggerUrl"].unwrap_string().clone()).collect()
	};

	println!("Connecting {}...", session_list[0]);
	let dc = RemoteCampus::connect(&session_list[0]).expect("Failed to connect to a session in the Headless Chrome");
	println!("  Connection established.");
	let pctrl = dc.wait_login_completion().expect("Failed waiting initial login completion").unwrap_or_else(move |mut e|
	{
		// println!("Logging-in required for DigitalCampus");
		println!("デジキャンへのログインが必要です。");
		if let Some(al) = autologin
		{
			println!("自動ログインの処理中です({})...", al[0]);
			e.set_login_info_fields(&al[0], &al[1]).unwrap();
			e.submit().expect("Error logging in").unwrap_or_else(|e|
			{
				// println!("** Failed to login to DigitalCampus. Check whether Student Number or password is correct **");
				println!("** デジキャンへのログインに失敗しました。学籍番号またはパスワードが正しいか確認してください。 **");
				process_login(e)
			})
		}
		else { process_login(e) }
	});
	println!("履修ページへアクセスしています...");
	let mut intersysmenu = pctrl.jump_into_intersys().unwrap().isolate_mainframe().unwrap();

	// 学生プロファイルと履修科目テーブル
	/*let mut cdetails = intersysmenu.interrupt().unwrap().jump_into_course_category().unwrap().isolate_mainframe_stealing_load().unwrap()
		.jump_into_course_details().unwrap();
	let profile = cdetails.parse_profile().unwrap();
	/*println!("=== 学生プロファイル ===");
	println!("** 学籍番号: {}", profile.id);
	println!("** 氏名: {}", profile.name);
	println!("** 学部/学年: {} {}", profile.course, profile.grade);
	println!("** セメスタ: {}", profile.semester);
	println!("** 住所: {}", profile.address.join(" "))*/
	println!("{}", serde_json::to_string(&profile).unwrap());
	println!("{}", serde_json::to_string(&cdetails.parse_course_table().unwrap()).unwrap());
	println!("{}", serde_json::to_string(&cdetails.parse_graduation_requirements_table().unwrap()).unwrap());*/

	// 出席率を取りたい
	let mut adetails = intersysmenu./*activate(cdetails.leave()).unwrap().*/jump_into_attendance_category().unwrap().isolate_mainframe_stealing_load().unwrap()
		.jump_into_details().unwrap();
	println!("{}", serde_json::to_string(&adetails.parse_current_year_table().unwrap()).unwrap());
	println!("{}", serde_json::to_string(&adetails.parse_attendance_rates().unwrap()).unwrap());
}

fn prompt(text: &str) -> String
{
	write!(std::io::stdout(), "{}>", text).unwrap(); std::io::stdout().flush().unwrap();
	let mut s = String::new();
	std::io::stdin().read_line(&mut s).unwrap(); s
}

// platform dependent - POSIX(Linux)
#[cfg(unix)]
extern crate termios;
#[cfg(unix)]
const STDIN_FD: std::os::unix::io::RawFd = 0;
#[cfg(unix)]
fn disable_echo()
{
	use termios::Termios;
	let mut tio = Termios::from_fd(STDIN_FD).unwrap();
	tio.c_lflag &= !termios::ECHO;
	termios::tcsetattr(STDIN_FD, termios::TCSANOW, &tio).unwrap();
}
#[cfg(unix)]
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
