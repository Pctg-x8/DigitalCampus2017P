
#![feature(iterator_step_by, box_syntax, const_fn)]

extern crate dc_web;
extern crate tokio_core;
extern crate serde_json;
extern crate hyper;
extern crate futures;
extern crate websocket;
extern crate colored;

use tokio_core::reactor::Core;
use futures::{Future, Stream};
use std::io::prelude::*;

use dc_web::headless_chrome::{Process as ChromeProcess, BrowserVersion, page, SessionInfo as ChromeSessionInfo};
use dc_web::{RemoteCampus, HomeMenuControl, NotificationListPage};

fn process_login(mut pctrl: dc_web::LoginPage) -> dc_web::HomePage
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

fn frame_navigated(e: &page::FrameNavigated)
{
	use colored::*;

	if let Some(n) = e.frame.name.as_ref() { println!("{} in {}", format!("FrameNavigated: {}", e.frame.url).bold(), n); }
	else { println!("{}", format!("FrameNavigated: {}", e.frame.url).bold()); }
}

fn main()
{
	println!("DigitalCampus 2017 Prototype");

	let autologin = std::env::args().nth(1).map(|s| s.split(":").map(ToOwned::to_owned).collect::<Vec<String>>());

	let chrome = ChromeProcess::run(9222, "https://dh.force.com/digitalCampus/campusHomepage").expect("Failed to launch the Headless Chrome");

	let mut tcore = Core::new().expect("Failed to initialize tokio-core");
	let client = hyper::Client::new(&tcore.handle());
	let ua_dc2017 = {
		let received = String::from_utf8_lossy(&tcore.run(chrome.get_version_async(&client).and_then(|res| res.body().concat2())).unwrap()).into_owned();
		let version_info: BrowserVersion = serde_json::from_str(&received).unwrap();
		println!("Headless Chrome: {} :: {}", version_info.browser, version_info.protocol_version);
		println!("  webkit: {}", version_info.webkit_version);
		println!("  user-agent: {}", version_info.user_agent);

		// Create UA String
		format!("DigitalCampus2017 w/ {}", version_info.user_agent)
	};
	let data =
	{
		let buffer = tcore.run(chrome.get_sessions_async(&client).and_then(|res| res.body().concat2())).unwrap();
		String::from_utf8_lossy(&buffer).into_owned()
	};
	let list_js: Vec<ChromeSessionInfo> = serde_json::from_str(&data).unwrap();
	let main_session = list_js[0].web_socket_debugger_url.unwrap();

	println!("Connecting {}...", main_session);
	let mut dc = RemoteCampus::connect(main_session, Some(&ua_dc2017)).expect("Failed to connect to a session in the Headless Chrome");
	println!("  Connection established.");
	dc.subscribe_frame_navigated(&frame_navigated);
	let mut pctrl = dc.check_login_completion().expect("Failed waiting initial login completion").unwrap_or_else(move |mut e|
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

	println!("{:?}", pctrl.acquire_notifications_latest().unwrap());
	println!("{:?}", pctrl.acquire_lecture_notifications_latest().unwrap());
	println!("{:?}", pctrl.acquire_feedback_sheets().unwrap());
	println!("{:?}", pctrl.acquire_homeworks().unwrap());

	let mut all_notifications = pctrl.access_all_notifications().unwrap();
	println!("{:?}", all_notifications.acquire_notifications().unwrap());

	all_notifications.access_intersys_blank().unwrap();
	println!("** Switching Session **");
	let data =
	{
		let buffer = tcore.run(chrome.get_sessions_async(&client).and_then(|res| res.body().concat2())).unwrap();
		String::from_utf8_lossy(&buffer).into_owned()
	};
	let list_js: Vec<ChromeSessionInfo> = serde_json::from_str(&data).unwrap();
	let intersys_session_url = list_js.into_iter().find(|x| x.url.contains("/CplanMenuWeb/"))
		.and_then(|x| x.web_socket_debugger_url)
		.expect("Unable to find internal system session");
	let mut dc_intersys = RemoteCampus::connect(&intersys_session_url, Some(&ua_dc2017))
		.expect("Failed to connect to a internal system session in the Headless Chrome");
	dc_intersys.subscribe_frame_navigated(&frame_navigated);
	let mut intersysmenu = unsafe { dc_web::CampusPlanEntryFrames::enter(dc_intersys) };
	intersysmenu.wait_frame_context(true).unwrap();

	// 学生プロファイルと履修科目テーブル
	let mut cdetails = intersysmenu.access_course_category().unwrap().access_details().unwrap();
	let profile = cdetails.parse_profile().unwrap();
	println!("{:?}", profile);
	println!("{:?}", cdetails.parse_course_table().unwrap());
	println!("{:?}", cdetails.parse_graduation_requirements_table().unwrap());

	// 出席率を取りたい
	let mut adetails = cdetails.access_attendance_category().unwrap().access_details().unwrap();
	// let mut adetails = intersysmenu.access_attendance_category().unwrap().access_details().unwrap();
	println!("{:?}", adetails.parse_current_year_table().unwrap());
	println!("{:?}", adetails.parse_attendance_rates().unwrap());

	let mut feedback_sheets = all_notifications.access_all_feedback_sheets().unwrap();
	println!("{:?}", feedback_sheets.acquire_notifications().unwrap());
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
