//! DigitalCampus Remote Controllers

#![allow(dead_code)]

use {headless_chrome, GenericResult};
use headless_chrome::{Event, RequestID};
use std::net::TcpStream;
use serde_json;
use serde_json::{Value as JValue, Map as JMap};
use std::marker::PhantomData;
use std::mem::{replace, transmute};
use chrono::prelude::*;

use headless_chrome::{page, runtime};
use headless_chrome::runtime::JSONTyping;
use jsquery as jsq;
use jsquery::QueryCombinator;

pub trait QueryValueType<T: Sized>: Sized { fn unwrap(self) -> T; }
impl QueryValueType<JMap<String, JValue>> for JValue
{
	fn unwrap(self) -> JMap<String, JValue> { jvDecomposite!{ self => object[v]: v } }
}
impl QueryValueType<Vec<JValue>> for JValue
{
	fn unwrap(self) -> Vec<JValue> { jvDecomposite!{ self => array[v]: v } }
}
impl QueryValueType<String> for JValue { fn unwrap(self) -> String { jvDecomposite!{ self => string[v]: v } } }

pub struct RemoteCampus { session: headless_chrome::Session<TcpStream, TcpStream>, request_id: RequestID }
impl RemoteCampus
{
	pub fn connect(addr: &str) -> GenericResult<Self>
	{
		let mut object = headless_chrome::Session::connect(addr).map(|session| RemoteCampus { session, request_id: 1 })?;
		object.session.page().enable(0)?; object.session.wait_result(0)?;
		object.session.dom().enable(0)?; object.session.wait_result(0)?;
		object.session.runtime().enable(0)?; object.session.wait_result(0)?;
		Ok(object)
	}
	fn new_request_id(&mut self) -> RequestID
	{
		let r = self.request_id; self.request_id += 1; r
	}
	pub fn subscribe_frame_navigated<S: headless_chrome::FrameNavigatedEventSubscriber>(&mut self, subscriber: &'static S)
	{
		self.session.subscribe_session_event(subscriber);
	}

	pub fn query(&mut self, context: Option<u64>, expression: &str) -> GenericResult<()>
	{
		let id = self.new_request_id();
		let q = if let Some(cid) = context
		{
			self.session.runtime().evaluate_in_sync(id, cid, expression)?
		}
		else
		{
			self.session.runtime().evaluate_sync(id, expression)?
		};
		if q.result.subtype == Some(headless_chrome::runtime::ObjectSubtype::Error)
		{
			// Error occured
			panic!("Error in querying browser: {:?}", q);
		}
		else { Ok(()) }
	}
	pub fn query_value(&mut self, context: Option<u64>, expression: &str) -> GenericResult<headless_chrome::runtime::RemoteObject>
	{
		let id = self.new_request_id();
		let q = if let Some(cid) = context
		{
			self.session.runtime().evaluate_value_in_sync(id, cid, expression)?
		}
		else
		{
			self.session.runtime().evaluate_value_sync(id, expression)?
		};
		if q.result.subtype == Some(headless_chrome::runtime::ObjectSubtype::Error)
		{
			// Error occured
			panic!("Error in querying value to browser: {:?}", q);
		}
		else { Ok(q.result) }
	}
	pub fn query_page_location(&mut self, cid: Option<u64>) -> GenericResult<String>
	{
		self.query_value(cid, "location.href").map(runtime::RemoteObject::assume_string)
	}
	pub fn is_in_login_page(&mut self) -> GenericResult<bool>
	{
		Ok(self.query_page_location(None)?.contains("/campuslogin"))
	}
	pub fn is_in_home(&mut self) -> GenericResult<bool>
	{
		Ok(self.query_page_location(None)?.contains("/campusHomepage"))
	}

	pub fn click_element(&mut self, context: Option<u64>, selector: &str) -> GenericResult<&mut Self>
	{
		self.query(context, &format!(r#"document.querySelector({:?}).click()"#, selector)).map(move |_| self)
	}
	pub fn click_nth_element(&mut self, context: Option<u64>, selector: &str, index: usize) -> GenericResult<&mut Self>
	{
		self.query(context, &format!(r#"document.querySelectorAll({:?})[{}].click()"#, selector, index)).map(move |_| self)
	}
	pub fn jump_to_anchor_href(&mut self, selector: &str) -> GenericResult<&mut Self>
	{
		let id = self.new_request_id(); let id2 = self.new_request_id();
		let intersys_link_attrs = self.session.dom().get_root_node_sync(id)?.query_selector(selector)?.attributes()?;
		let href_index = intersys_link_attrs.iter().position(|s| s == "href").unwrap() + 1;
		self.session.page().navigate_sync(id2, intersys_link_attrs[href_index].as_str().unwrap()).map(move |_| self)
	}
	pub fn jump_to_nth_anchor_href(&mut self, selector: &str, index: usize) -> GenericResult<&mut Self>
	{
		let id = self.new_request_id(); let id2 = self.new_request_id();
		let intersys_link_attrs = self.session.dom().get_root_node_sync(id)?.query_selector_nth(selector, index)?.attributes()?;
		let href_index = intersys_link_attrs.iter().position(|s| s == "href").unwrap() + 1;
		self.session.page().navigate_sync(id2, intersys_link_attrs[href_index].as_str().unwrap()).map(move |_| self)
	}

	/// synchronize page
	pub fn wait_loading(&mut self) -> GenericResult<&mut Self>
	{
		self.session.wait_event::<headless_chrome::page::LoadEventFired>().map(move |_| self)
	}
}

/// ログインページ
pub struct LoginPage { remote: RemoteCampus }
impl RemoteCampus { pub unsafe fn assume_login(self) -> LoginPage { LoginPage { remote: self } } }
impl LoginPage
{
	const FORM_NAME_ID:       &'static str = "loginPage:formId:j_id33";
	const FORM_NAME_PASSWORD: &'static str = "loginPage:formId:j_id34";
	/// ログインIDフィールドを設定
	pub fn set_login_id_field(&mut self, login_id: &str) -> GenericResult<&mut Self>
	{
		let id = self.remote.new_request_id();
		self.remote.session.runtime().evaluate_sync(id, &format!(r#"document.querySelector('input[name={:?}]').value = {:?};"#, Self::FORM_NAME_ID, login_id))
			.map(move |_| self)
	}
	/// パスワードフィールドを設定
	pub fn set_password_field(&mut self, pass: &str) -> GenericResult<&mut Self>
	{
		let id = self.remote.new_request_id();
		self.remote.session.dom().get_root_node_sync(id).unwrap().query_selector(&format!(r#"input[name={:?}]"#, Self::FORM_NAME_PASSWORD))?.focus()?;
		for c in pass.trim_right().chars()
		{
			let id = self.remote.new_request_id();
			self.remote.session.input().dispatch_key_event_sync(id, headless_chrome::input::KeyEvent::Char, Some(&c.to_string())).unwrap();
		}
		Ok(self)
	}
	/// IDとパスワードを設定
	pub fn set_login_info_fields(&mut self, login_id: &str, pass: &str) -> GenericResult<&mut Self>
	{
		self.set_login_id_field(login_id)?.set_password_field(pass)
	}
	/// ログイン実行
	pub fn submit(mut self) -> GenericResult<Result<HomePage, LoginPage>>
	{
		self.remote.session.input().dispatch_key_event(0, headless_chrome::input::KeyEvent::Char, Some("\r"))?;
		self.remote.wait_loading()?;
		self.remote.check_login_completion()
	}
}

/// メインページ
pub struct HomePage { remote: RemoteCampus }
impl RemoteCampus
{
	pub unsafe fn assume_home(self) -> HomePage { HomePage { remote: self } }
	pub fn check_login_completion(mut self) -> GenericResult<Result<HomePage, LoginPage>>
	{
		if self.is_in_home()? { Ok(Ok(unsafe { self.assume_home() })) }
		else if self.is_in_login_page()? { Ok(Err(unsafe { self.assume_login() })) }
		else { self.wait_loading()?; self.check_login_completion() }
	}
}

pub trait ToplevelPageControl
{
	/// リモートコントローラ
	fn remote_ctrl(&mut self) -> &mut RemoteCampus;
	/// コントロール奪取
	fn transfer_control(self) -> RemoteCampus;
}
/// ホームページのメインメニュー操作
pub trait HomeMenuControl : ToplevelPageControl + Sized
{
	const INTERSYS_LINK_PATH: &'static str = "#gnav ul li.menuBlock ul li:first-child a";
	const NOTIFICATIONS_LINK_PATH: &'static str = "#gnav ul li:nth-child(2) a";
	const LECTURE_CATEGORY_LINKS_PATH: &'static str = "#gnav ul li:nth-child(3) ul li a";

	/// "履修・成績・出席"リンクへ
	/// 将来的にmenuBlockクラスが複数出てきたらまた考えます
	fn access_intersys(mut self) -> GenericResult<CampusPlanEntryFrames>
	{
		self.remote_ctrl().jump_to_anchor_href(Self::INTERSYS_LINK_PATH)?;
		let mut r = unsafe { CampusPlanFrames::enter(self.transfer_control()) };
		r.wait_frame_context(true)?; Ok(r)
	}
	/// "履修・成績・出席"リンクへ(別セッションで立ち上がる)
	/// 将来的にmenuBlockクラスが複数出てきたらまた考えます
	fn access_intersys_blank(&mut self) -> GenericResult<()>
	{
		self.remote_ctrl().click_element(None, Self::INTERSYS_LINK_PATH).map(drop)
	}

	/// すべてのお知らせが見れるページに飛ぶ
	fn access_all_notifications(mut self) -> GenericResult<AllNotificationsPage>
	{
		// self.remote.jump_to_anchor_href(&format!("{}:nth-child(1) {}", Self::NEWSBOX_LIST, Self::TOALL_LINK_PATH))?.wait_loading()?;
		self.remote_ctrl().click_element(None, Self::NOTIFICATIONS_LINK_PATH)?.wait_loading()?;
		Ok(AllNotificationsPage { remote: self.transfer_control() })
	}
	/// すべての休講・補講・教室変更一覧が見れるページに飛ぶ
	fn access_all_class_notifications(mut self) -> GenericResult<AllClassNotificationsPage>
	{
		self.remote_ctrl().click_nth_element(None, Self::LECTURE_CATEGORY_LINKS_PATH, 1)?.wait_loading()?;
		Ok(AllClassNotificationsPage { remote: self.transfer_control() })
	}
	/// すべての回答待ちフィードバックシート一覧が見れるページに飛ぶ
	fn access_all_feedback_sheets(mut self) -> GenericResult<AllFeedbackSheetNotificationsPage>
	{
		self.remote_ctrl().click_nth_element(None, Self::LECTURE_CATEGORY_LINKS_PATH, 2)?.wait_loading()?;
		Ok(AllFeedbackSheetNotificationsPage { remote: self.transfer_control() })
	}
	/// すべての回答待ち課題一覧が見れるページに飛ぶ
	fn access_all_homeworks(mut self) -> GenericResult<AllHomeworkNotificationsPage>
	{
		self.remote_ctrl().click_nth_element(None, Self::LECTURE_CATEGORY_LINKS_PATH, 3)?.wait_loading()?;
		Ok(AllHomeworkNotificationsPage { remote: self.transfer_control() })
	}
	/// すべての授業資料一覧が見れるページに飛ぶ
	fn access_all_lecture_notes(mut self) -> GenericResult<AllLectureNotesPage>
	{
		self.remote_ctrl().click_nth_element(None, Self::LECTURE_CATEGORY_LINKS_PATH, 4)?.wait_loading()?;
		Ok(AllLectureNotesPage { remote: self.transfer_control() })
	}
	/// すべての授業関連の連絡一覧が見れるページに飛ぶ
	fn access_all_lecture_notifications(mut self) -> GenericResult<AllLectureNotificationsPage>
	{
		self.remote_ctrl().click_nth_element(None, Self::LECTURE_CATEGORY_LINKS_PATH, 5)?.wait_loading()?;
		Ok(AllLectureNotificationsPage { remote: self.transfer_control() })
	}
}
impl ToplevelPageControl for HomePage
{
	fn remote_ctrl(&mut self) -> &mut RemoteCampus { &mut self.remote } fn transfer_control(self) -> RemoteCampus { self.remote }
}
impl ToplevelPageControl for AllNotificationsPage
{
	fn remote_ctrl(&mut self) -> &mut RemoteCampus { &mut self.remote } fn transfer_control(self) -> RemoteCampus { self.remote }
}
impl ToplevelPageControl for AllClassNotificationsPage
{
	fn remote_ctrl(&mut self) -> &mut RemoteCampus { &mut self.remote } fn transfer_control(self) -> RemoteCampus { self.remote }
}
impl ToplevelPageControl for AllLectureNotesPage
{
	fn remote_ctrl(&mut self) -> &mut RemoteCampus { &mut self.remote } fn transfer_control(self) -> RemoteCampus { self.remote }
}
impl ToplevelPageControl for AllLectureNotificationsPage
{
	fn remote_ctrl(&mut self) -> &mut RemoteCampus { &mut self.remote } fn transfer_control(self) -> RemoteCampus { self.remote }
}
impl ToplevelPageControl for AllFeedbackSheetNotificationsPage
{
	fn remote_ctrl(&mut self) -> &mut RemoteCampus { &mut self.remote } fn transfer_control(self) -> RemoteCampus { self.remote }
}
impl ToplevelPageControl for AllHomeworkNotificationsPage
{
	fn remote_ctrl(&mut self) -> &mut RemoteCampus { &mut self.remote } fn transfer_control(self) -> RemoteCampus { self.remote }
}
impl HomeMenuControl for HomePage {}
impl HomeMenuControl for AllNotificationsPage {}
impl HomeMenuControl for AllClassNotificationsPage {}
impl HomeMenuControl for AllLectureNotesPage {}
impl HomeMenuControl for AllLectureNotificationsPage {}
impl HomeMenuControl for AllFeedbackSheetNotificationsPage {}
impl HomeMenuControl for AllHomeworkNotificationsPage {}

/// トップコンテンツ取得
impl HomePage
{
	const NEWSBOX_LIST: &'static str = "#mainContents .homeNewsBox";
	const NEWSBOX_CONTENT_ROWS: &'static str = "table:nth-child(2) tr.pointer";
	const TOALL_LINK_PATH: &'static str = ".toAll a";
	const COMMONFN_TRANSLATE_NS: &'static str = r#"function translateNotificationState(s) {
		switch(s) {
		case "未読": return "Unread"; case "既読": return "Read";
		case "未回答": return "Unanswered"; case "回答済": return "Answered";
		case "未提出": return "Unsubmitted"; case "提出済": return "Submitted";
		default: console.assert(0);
		}
	}"#;
	
	fn query_all_row_contents<Source: QueryCombinator>(row: Source)
		-> jsq::Mapping<jsq::QuerySelectorAll<Source>, jsq::Closure<'static, jsq::CustomExpression<jsq::types::String>>>
		where Source::ValueTy: jsq::types::QueryableElements
	{
		row.query_selector_all("td".into()).map_auto("x", jsq::CustomExpression::<jsq::types::String>("x.textContent.trim()".into(), PhantomData))
	}
	fn query_rows(index1: usize) -> jsq::QuerySelectorAll<jsq::Document>
	{
		jsq::Document.query_selector_all(format!("{}:nth-child({}) {}", Self::NEWSBOX_LIST, index1, Self::NEWSBOX_CONTENT_ROWS))
	}
	fn gen_object(values: &[(&str, &str)]) -> jsq::CustomExpression<jsq::types::Object>
	{
		jsq::CustomExpression(format!("({{ {} }})",
			values.into_iter().map(|&(ref k, ref v)| format!("{}: {}", k, v)).collect::<Vec<String>>().join(",")), PhantomData)
	}
	fn reformat_date(expr: &str) -> String
	{
		format!(r#"{}.replace(/(\d+)\/(\d+)\/(\d+)/, "$1-$2-$3T00:00:00Z")"#, expr)
	}
	fn reformat_datetime(expr: &str) -> String
	{
		format!(r#"{}.replace(/(\d+)\/(\d+)\/(\d+)\s*(\d+:\d+)/, "$1-$2-$3T$4:00Z")"#, expr)
	}

	/// 最新のお知らせ(5件?)を取得
	pub fn acquire_notifications_latest(&mut self) -> GenericResult<Vec<Notification>>
	{
		let q: String = self.remote.query_value(None, &Self::query_rows(1).map_auto("r",
			Self::query_all_row_contents(jsq::CustomExpression::<jsq::types::Element>("r".into(), PhantomData)).map_value_auto("cells", jsqGenObject!{
				category: "cells[0]", date: &Self::reformat_date("cells[1]"), priority: "cells[2]", title: "cells[3]", from: "cells[4]",
				state: "translateNotificationState(cells[5])", onClickScript: r#"r.getAttribute("onclick").substring("javascript:".length)"#
			})).stringify().with_header(Self::COMMONFN_TRANSLATE_NS))?.assume();
		Ok(serde_json::from_str(&q).expect("Protocol Corruption"))
	}
	/// 授業関連の最新のお知らせ(〜3件?)を取得
	pub fn acquire_lecture_notifications_latest(&mut self) -> GenericResult<Vec<ClassNotification>>
	{
		let q: String = self.remote.query_value(None, &Self::query_rows(2).map_auto("r",
			Self::query_all_row_contents(jsq::CustomExpression::<jsq::types::Element>("r".into(), PhantomData)).map_value_auto("cells", jsqGenObject!{
				category: "cells[0]", date: &Self::reformat_date("cells[1]"), priority: "cells[2]", lectureTitle: "cells[3]", title: "cells[4]",
				state: "translateNotificationState(cells[5])", onClickScript: r#"r.getAttribute("onclick").substring("javascript:".length)"#
			})).stringify().with_header(Self::COMMONFN_TRANSLATE_NS))?.assume();
		Ok(serde_json::from_str(&q).expect("Protocol Corruption"))
	}
	/// フィードバックシート回答待ちリストの取得
	pub fn acquire_feedback_sheets(&mut self) -> GenericResult<Vec<FeedbackSheetNotification>>
	{
		let q: String = self.remote.query_value(None, &Self::query_rows(3).map_auto("r",
			Self::query_all_row_contents(jsq::CustomExpression::<jsq::types::Element>("r".into(), PhantomData)).map_value_auto("cells", jsqGenObject!{
				lectureDate: &Self::reformat_date("cells[0]"), lectureTitle: "cells[1]",
				time: "parseInt(cells[2].replace(/[０-９]/g, x => String.fromCharCode(x.charCodeAt(0) - 65248)))",
				deadline: &Self::reformat_datetime("cells[3]"), state: "translateNotificationState(cells[4])",
				onClickScript: r#"r.getAttribute("onclick").substring("javascript:".length)"#
			})).stringify().with_header(Self::COMMONFN_TRANSLATE_NS))?.assume();
		Ok(serde_json::from_str(&q).expect("Protocol Corruption"))
	}
	/// 課題回答待ちリストの取得
	pub fn acquire_homeworks(&mut self) -> GenericResult<Vec<HomeworkNotification>>
	{
		let q: String = self.remote.query_value(None, &Self::query_rows(4).map_auto("r",
			Self::query_all_row_contents(jsq::CustomExpression::<jsq::types::Element>("r".into(), PhantomData)).map_value_auto("cells", jsqGenObject!{
				date: &Self::reformat_date("cells[0]"), lectureTitle: "cells[1]", title: "cells[2]",
				deadline: &Self::reformat_datetime("cells[3]"), state: "translateNotificationState(cells[4])",
				onClickScript: r#"r.getAttribute("onclick").substring("javascript:".length)"#
			})).stringify().with_header(Self::COMMONFN_TRANSLATE_NS))?.assume();
		Ok(serde_json::from_str(&q).expect("Protocol Corruption"))
	}
}
/// お知らせ一覧のページ
pub struct AllNotificationsPage { remote: RemoteCampus }
/// 休講/補講/教室変更お知らせ一覧のページ
pub struct AllClassNotificationsPage { remote: RemoteCampus }
/// 授業資料一覧のページ
pub struct AllLectureNotesPage { remote: RemoteCampus }
/// その他講義関連の連絡一覧のページ
pub struct AllLectureNotificationsPage { remote: RemoteCampus }
/// フィードバックシート回答待ち一覧のページ
pub struct AllFeedbackSheetNotificationsPage { remote: RemoteCampus }
/// 課題回答待ち一覧のページ
pub struct AllHomeworkNotificationsPage { remote: RemoteCampus }

/// 通知一覧ページの共通実装
pub trait NotificationListPage : ToplevelPageControl
{
	/// 自身が返す通知行の型
	type NotificationTy : ::serde::de::DeserializeOwned;
	/// 通知行オブジェクトを構成するJSQ Fragment("cells"にデータが入っているので、それを加工するJSQ Fragment)
	fn jsqf_notification_gen() -> jsq::ObjectConstructor<'static, 'static>;

	/// すべての通知を取得
	fn acquire_notifications(&mut self) -> GenericResult<Vec<Self::NotificationTy>>
	{
		let row = jsqCustomExpr!([jsq::types::Element] "r").query_selector_all("td".into())
			.map_auto("x", jsqCustomExpr!([jsq::types::String] "x.textContent.trim()"))
			.map_value_auto("cells", Self::jsqf_notification_gen()).into_closure("r");
		let q = jsq::Document.query_selector_all("#mainContents .homeNewsBox .pointer".into()).map(row).stringify();
		let qv: String = self.remote_ctrl().query_value(None, &q.with_header(HomePage::COMMONFN_TRANSLATE_NS))?.assume();
		Ok(serde_json::from_str(&qv).expect("Protocol Corruption"))
	}
}
const REFORMAT_DATE_CELLS_1: &'static str = r#"cells[1].replace(/(\d+)\/(\d+)\/(\d+)/, "$1-$2-$3T00:00:00Z")"#;
impl NotificationListPage for AllNotificationsPage
{
	type NotificationTy = Notification;
	fn jsqf_notification_gen() -> jsq::ObjectConstructor<'static, 'static>
	{
		jsqGenObject!{
			category: "cells[0]", date: REFORMAT_DATE_CELLS_1, priority: "cells[2]", title: "cells[3]", from: "cells[4]",
			state: "translateNotificationState(cells[5])", onClickScript: r#"r.getAttribute("onclick").substring("javascript:".length)"#
		}
	}
}
impl NotificationListPage for AllClassNotificationsPage
{
	type NotificationTy = ClassNotification;
	fn jsqf_notification_gen() -> jsq::ObjectConstructor<'static, 'static>
	{
		jsqGenObject!{
			category: "cells[0]", date: REFORMAT_DATE_CELLS_1, priority: "cells[2]", lectureTitle: "cells[3]", title: "cells[4]",
			state: "translateNotificationState(cells[5])", onClickScript: r#"r.getAttribute("onclick").substring("javascript:".length)"#
		}
	}
}
const REFORMAT_DATE_CELLS_0: &'static str = r#"cells[0].replace(/(\d+)\/(\d+)\/(\d+)/, "$1-$2-$3T00:00:00Z")"#;
impl NotificationListPage for AllLectureNotesPage
{
	type NotificationTy = LectureNotification;
	fn jsqf_notification_gen() -> jsq::ObjectConstructor<'static, 'static>
	{
		jsqGenObject!{
			date: REFORMAT_DATE_CELLS_0, priority: "cells[1]", lectureTitle: "cells[2]", title: "cells[3]",
			state: "translateNotificationState(cells[4])", onClickScript: r#"r.getAttribute("onclick").substring("javascript:".length)"#
		}
	}
}
impl NotificationListPage for AllLectureNotificationsPage
{
	type NotificationTy = LectureNotification;
	fn jsqf_notification_gen() -> jsq::ObjectConstructor<'static, 'static>
	{
		jsqGenObject!{
			date: REFORMAT_DATE_CELLS_0, priority: "cells[1]", lectureTitle: "cells[2]", title: "cells[3]",
			state: "translateNotificationState(cells[4])", onClickScript: r#"r.getAttribute("onclick").substring("javascript:".length)"#
		}
	}
}
const REFORMAT_DATE_CELLS_3: &'static str = r#"cells[3].replace(/(\d+)\/(\d+)\/(\d+)/, "$1-$2-$3T00:00:00Z")"#;
impl NotificationListPage for AllFeedbackSheetNotificationsPage
{
	type NotificationTy = FeedbackSheetNotification;
	fn jsqf_notification_gen() -> jsq::ObjectConstructor<'static, 'static>
	{
		jsqGenObject!{
			lectureDate: REFORMAT_DATE_CELLS_0, lectureTitle: "cells[1]",
			time: "parseInt(cells[2].replace(/[０-９]/g, x => String.fromCharCode(x.charCodeAt(0) - 65248)))",
			deadline: REFORMAT_DATE_CELLS_3, state: "translateNotificationState(cells[4])",
			onClickScript: r#"r.getAttribute("onclick").substring("javascript:".length)"#
		}
	}
}
impl NotificationListPage for AllHomeworkNotificationsPage
{
	type NotificationTy = HomeworkNotification;
	fn jsqf_notification_gen() -> jsq::ObjectConstructor<'static, 'static>
	{
		jsqGenObject!{
			date: REFORMAT_DATE_CELLS_0, lectureTitle: "cells[1]", title: "cells[2]",
			deadline: REFORMAT_DATE_CELLS_3, state: "translateNotificationState(cells[4])",
			onClickScript: r#"r.getAttribute("onclick").substring("javascript:".length)"#
		}
	}
}

/// お知らせ行
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)] #[serde(rename_all = "camelCase")]
pub struct Notification
{
	pub category: String, pub date: DateTime<Utc>, pub priority: String, pub title: String, pub from: String,
	pub state: NotificationState, pub on_click_script: String
}
/// 講義関連お知らせ行
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)] #[serde(rename_all = "camelCase")]
pub struct ClassNotification
{
	pub category: String, pub date: DateTime<Utc>, pub priority: String, pub lecture_title: String, pub title: String,
	pub state: NotificationState, pub on_click_script: String
}
/// 講義連絡行
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)] #[serde(rename_all = "camelCase")]
pub struct LectureNotification
{
	pub date: DateTime<Utc>, pub priority: String, pub lecture_title: String, pub title: String,
	pub state: NotificationState, pub on_click_script: String
}
/// フィードバックシート回答待ち行
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)] #[serde(rename_all = "camelCase")]
pub struct FeedbackSheetNotification
{
	pub lecture_date: DateTime<Utc>, pub lecture_title: String, pub time: u32, pub deadline: DateTime<Utc>,
	pub state: NotificationState, pub on_click_script: String
}
/// 課題回答待ち行
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)] #[serde(rename_all = "camelCase")]
pub struct HomeworkNotification
{
	pub date: DateTime<Utc>, pub lecture_title: String, pub title: String, pub deadline: DateTime<Utc>,
	pub state: NotificationState, pub on_click_script: String
}
/// 閲覧状態
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum NotificationState
{
	Unread, Read, Unanswered, Answered, Unsubmitted, Submitted
}

#[derive(Debug, PartialEq, Eq)]
pub enum ScriptContextState { Unloaded, Empty(String), Context(String, u64) }
impl ScriptContextState
{
	fn frameid(&self) -> Option<&str>
	{
		match self { &ScriptContextState::Context(ref s, _) | &ScriptContextState::Empty(ref s) => Some(s), _ => None }
	}
	fn contextid(&self) -> Option<u64>
	{
		match self
		{
			&ScriptContextState::Context(_, c) => Some(c), _ => None
		}
	}
	
	fn navigated(&mut self, fid: String)
	{
		if let &mut ScriptContextState::Empty(ref mut s) = self { *s = fid; }
		else if let ScriptContextState::Context(s, ccid) = replace(self, ScriptContextState::Unloaded)
		{
			*self = if s == fid { ScriptContextState::Context(s, ccid) } else { ScriptContextState::Empty(fid) };
		}
		else { *self = ScriptContextState::Empty(fid); }
	}
	fn try_attach_context(&mut self, fid: &str, cid: u64)
	{
		if let &mut ScriptContextState::Context(ref s, ref mut ccid) = self
		{
			if s == fid { *ccid = cid; }
		}
		else if let ScriptContextState::Empty(s) = replace(self, ScriptContextState::Unloaded)
		{
			*self = if s == fid { ScriptContextState::Context(s, cid) } else { ScriptContextState::Empty(s) };
		}
	}
	fn detach_context(&mut self)
	{
		*self = match replace(self, ScriptContextState::Unloaded)
		{
			ScriptContextState::Context(s, _) => ScriptContextState::Empty(s),
			c => c
		};
	}
}

#[cfg(feature = "verbose")] use colored::*;

trait Breakability { fn require_break(self) -> bool; }
impl Breakability for () { fn require_break(self) -> bool { false } }
impl Breakability for bool { fn require_break(self) -> bool { self } }
macro_rules! SessionEventLoop
{
	{ __SessionMatcher($name: expr, $params: expr) $($ee: ident)::+ => break } =>
	{
		if $name == $($ee::)+METHOD_NAME { break; }
	};
	{ __SessionMatcher($name: expr, $params: expr) $($ee: ident)::+ => $x: expr } =>
	{
		if $name == $($ee::)+METHOD_NAME
		{
			if ($x)(serde_json::from_value::<$($ee)::+>($params)?).require_break() { break; }
		}
	};
	{ __SessionMatcher($name: expr, $params: expr) $($ee: ident)::+ => break; $($rest: tt)* } =>
	{
		if $name == $($ee::)+METHOD_NAME { break; }
		else { SessionEventLoop!{ __SessionMatcher($name, $params) $($rest)* } }
	};
	{ __SessionMatcher($name: expr, $params: expr) $($ee: ident)::+ => $x: expr; $($rest: tt)* } =>
	{
		if $name == $($ee::)+METHOD_NAME
		{
			if ($x)(serde_json::from_value::<$($ee)::+>($params)?).require_break() { break; }
		}
		else { SessionEventLoop!{ __SessionMatcher($name, $params) $($rest)* } }
	};
	($session: expr; { $($content: tt)* }) =>
	{
		loop
		{
			let s = $session.wait_text()?;
			#[cfg(feature = "verbose")] println!("{}", format!("<<-- [SessionEventLoop]Received: {}", s).blue().bold());
			let obj: headless_chrome::SessionReceiveEvent = ::serde_json::from_str(&s)?;
			match obj
			{
				headless_chrome::SessionReceiveEvent::Method { method: name, params } =>
				{
					SessionEventLoop!{ __SessionMatcher(name, params) $($content)* }
				},
				e@headless_chrome::SessionReceiveEvent::Error { .. } => return Err(e.error_text().unwrap().into()),
				_ => ()
			}
		}
	}
}

/// CampusPlan フレームページ
pub struct CampusPlanFrames<MainFrameCtrlTy: PageControl, MenuFrameCtrlTy: PageControl>
{
	remote: RemoteCampus, ph: PhantomData<(MainFrameCtrlTy, MenuFrameCtrlTy)>,
	ctx_main_frame: ScriptContextState, ctx_menu_frame: ScriptContextState
}
impl<MainFrameCtrlTy: PageControl, MenuFrameCtrlTy: PageControl> CampusPlanFrames<MainFrameCtrlTy, MenuFrameCtrlTy>
{
	pub unsafe fn enter(remote: RemoteCampus) -> Self
	{
		CampusPlanFrames { remote, ph: PhantomData, ctx_main_frame: ScriptContextState::Unloaded, ctx_menu_frame: ScriptContextState::Unloaded }
	}
	fn continue_enter<NewMainFrameCtrlTy: PageControl, NewMenuFrameCtrlTy: PageControl>(self) -> CampusPlanFrames<NewMainFrameCtrlTy, NewMenuFrameCtrlTy>
	{
		unsafe { transmute(self) }
	}

	fn is_blank_main(&mut self) -> GenericResult<bool>
	{
		let cid = self.main_frame_context();
		self.remote.query_page_location(Some(cid)).map(|l| l.contains("/blank.html"))
	}
}

/// Context ops
impl<MainFrameCtrlTy: PageControl, MenuFrameCtrlTy: PageControl> CampusPlanFrames<MainFrameCtrlTy, MenuFrameCtrlTy>
{
	/// フレームのロードを待つ
	pub fn wait_frame_context(&mut self, wait_for_menu_context: bool) -> GenericResult<&mut Self>
	{
		let (mut main_completion, mut menu_completion) = (false, !wait_for_menu_context);

		SessionEventLoop!(self.remote.session;
		{
			page::FrameNavigatedOwned => |e: page::FrameNavigatedOwned|
			{
				self.remote.session.dispatch_frame_navigated(&e.borrow());
				match e.frame.name.as_ref().map(|s| s as &str)
				{
					Some("MainFrame") => { self.ctx_main_frame.navigated(e.frame.id); },
					Some("MenuFrame") => { self.ctx_menu_frame.navigated(e.frame.id); },
					_ => ()
				}
			};
			runtime::ExecutionContextCreated => |e: runtime::ExecutionContextCreated|
			{
				if let Some(aux) = e.context.aux_data
				{
					if let Some(fid) = aux.get("frameId").and_then(JValue::as_str)
					{
						self.ctx_main_frame.try_attach_context(fid, e.context.id);
						self.ctx_menu_frame.try_attach_context(fid, e.context.id);
					}
				}
			};
			runtime::ExecutionContextDestroyed => |e: runtime::ExecutionContextDestroyed|
			{
				if Some(e.execution_context_id) == self.ctx_main_frame.contextid() { self.ctx_main_frame.detach_context(); }
				if Some(e.execution_context_id) == self.ctx_menu_frame.contextid() { self.ctx_menu_frame.detach_context(); }
			};
			runtime::ExecutionContextsCleared => |_|
			{
				self.ctx_main_frame.detach_context();
				self.ctx_menu_frame.detach_context();
			};
			page::FrameStoppedLoadingOwned => |e: page::FrameStoppedLoadingOwned|
			{
				main_completion = main_completion || self.ctx_main_frame.frameid() == Some(&e.frame_id);
				menu_completion = menu_completion || self.ctx_menu_frame.frameid() == Some(&e.frame_id);
				main_completion && menu_completion
			}
		});
		Ok(self)
	}
	fn main_frame_context(&self) -> u64
	{
		self.ctx_main_frame.contextid().expect("ExecutionContext for MainFrame has not been created yet")
	}
	fn menu_frame_context(&self) -> u64
	{
		self.ctx_menu_frame.contextid().expect("ExecutionContext for MenuFrame has not been created yet")
	}
}
pub type CampusPlanEntryFrames      = CampusPlanFrames<CampusPlanEntryPage,      EmptyMenu>;
pub type CampusPlanCourseFrames     = CampusPlanFrames<CampusPlanCoursePage,     StudentMenu>;
pub type CampusPlanSyllabusFrames   = CampusPlanFrames<CampusPlanSyllabusPage,   StudentMenu>;
pub type CampusPlanAttendanceFrames = CampusPlanFrames<CampusPlanAttendancePage, StudentMenu>;
pub type CampusPlanCourseDetailsFrames     = CampusPlanFrames<CourseDetailsPage,     StudentMenu>;
pub type CampusPlanAttendanceDetailsFrames = CampusPlanFrames<AttendanceDetailsPage, StudentMenu>;

/// Tag(メニューなし)
pub enum EmptyMenu {}
/// Tag(学生用メニュー)
pub enum StudentMenu {}
/// Tag(CampusPlanのエントリーページを表す)
pub enum CampusPlanEntryPage {}
/// コンテンツ操作に関わる
impl CampusPlanEntryFrames
{
	const COURSE_CATEGORY_LINK_ID:     &'static str = "#dgSystem__ctl2_lbtnSystemName";
	#[allow(dead_code)]
	const SYLLABUS_CATEGORY_LINK_ID:   &'static str = "#dgSystem__ctl3_lbtnSystemName";
	const ATTENDANCE_CATEGORY_LINK_ID: &'static str = "#dgSystem__ctl4_lbtnSystemName";

	/// 履修関係セクションへ
	pub fn access_course_category(mut self) -> GenericResult<CampusPlanCourseFrames>
	{
		let rctx = Some(self.main_frame_context());
		self.remote.click_element(rctx, Self::COURSE_CATEGORY_LINK_ID)?;
		let mut r = unsafe { CampusPlanFrames::enter(self.remote) };
		r.wait_frame_context(true)?;
		while r.is_blank_main()? { r.wait_frame_context(true)?; }
		Ok(r)
	}
	/// Webシラバスセクションへ
	#[allow(dead_code)]
	pub fn access_syllabus_category(mut self) -> GenericResult<CampusPlanSyllabusFrames>
	{
		let rctx = Some(self.main_frame_context());
		self.remote.click_element(rctx, Self::SYLLABUS_CATEGORY_LINK_ID)?;
		let mut r = unsafe { CampusPlanFrames::enter(self.remote) };
		r.wait_frame_context(true)?;
		while r.is_blank_main()? { r.wait_frame_context(true)?; }
		Ok(r)
	}
	/// 出欠関係セクションへ
	pub fn access_attendance_category(mut self) -> GenericResult<CampusPlanAttendanceFrames>
	{
		let rctx = Some(self.main_frame_context());
		self.remote.click_element(rctx, Self::ATTENDANCE_CATEGORY_LINK_ID)?;
		let mut r = unsafe { CampusPlanFrames::enter(self.remote) };
		r.wait_frame_context(true)?;
		while r.is_blank_main()? { r.wait_frame_context(true)?; }
		Ok(r)
	}
}
/// Tag(CampusPlanの履修関係メニューページを表す)
pub enum CampusPlanCoursePage { }
impl CampusPlanCourseFrames
{
	const DETAILS_LINK_ID: &'static str = "#dgSystem__ctl2_lbtnPage";
	/// 履修チェック結果の確認ページへ
	/// * 履修登録期間中はこれだと動かないかもしれない
	pub fn access_details(mut self) -> GenericResult<CampusPlanCourseDetailsFrames>
	{
		let rctx = Some(self.main_frame_context());
		self.remote.click_element(rctx, Self::DETAILS_LINK_ID)?;
		self.wait_frame_context(false)?; Ok(self.continue_enter())
	}
}
/// 未実装
#[allow(dead_code)]
pub enum CampusPlanSyllabusPage { }
pub enum CampusPlanAttendancePage { }
impl CampusPlanAttendanceFrames
{
	const DETAILS_LINK_ID: &'static str = "#dgSystem__ctl2_lbtnPage";
	/// 出欠状況参照ページへ
	pub fn access_details(mut self) -> GenericResult<CampusPlanAttendanceDetailsFrames>
	{
		let rctx = Some(self.main_frame_context());
		self.remote.click_element(rctx, Self::DETAILS_LINK_ID)?;
		self.wait_frame_context(false)?; Ok(self.continue_enter())
	}
}
pub trait PageControl: Sized {}
impl PageControl for CampusPlanEntryPage  {}
impl PageControl for CampusPlanCoursePage {}
impl PageControl for CampusPlanSyllabusPage {}
impl PageControl for CampusPlanAttendancePage {}
impl PageControl for CourseDetailsPage {}
impl PageControl for AttendanceDetailsPage {}
impl PageControl for EmptyMenu {}
impl PageControl for StudentMenu {}

/// 学生用メニューコントロール
impl<MainFramePageTy: PageControl> CampusPlanFrames<MainFramePageTy, StudentMenu>
{
	const COURSE_LINK_ID: &'static str     = "#dtlstMenu__ctl0_lbtnSystemName";
	#[allow(dead_code)]
	const SYLLABUS_LINK_ID: &'static str   = "#dtlstMenu__ctl1_lbtnSystemName";
	const ATTENDANCE_LINK_ID: &'static str = "#dtlstMenu__ctl2_lbtnSystemName";

	/// 履修申請カテゴリへ
	pub fn access_course_category(mut self) -> GenericResult<CampusPlanCourseFrames>
	{
		let rctx = Some(self.menu_frame_context());
		self.remote.click_element(rctx, Self::COURSE_LINK_ID)?;
		self.wait_frame_context(false)?; Ok(self.continue_enter())
	}
	/// シラバスカテゴリへ(未実装)
	#[allow(dead_code)]
	pub fn access_syllabus_category(mut self) -> GenericResult<CampusPlanSyllabusFrames>
	{
		let rctx = Some(self.menu_frame_context());
		self.remote.click_element(rctx, Self::SYLLABUS_LINK_ID)?;
		self.wait_frame_context(false)?; Ok(self.continue_enter())
	}
	/// 出欠カテゴリへ
	pub fn access_attendance_category(mut self) -> GenericResult<CampusPlanAttendanceFrames>
	{
		let rctx = Some(self.menu_frame_context());
		self.remote.click_element(rctx, Self::ATTENDANCE_LINK_ID)?;
		self.wait_frame_context(false)?; Ok(self.continue_enter())
	}
}

/// 履修確認ページ
pub enum CourseDetailsPage { }
/// 学生プロファイル/履修データの解析周り
impl CampusPlanCourseDetailsFrames
{
	/// 学生プロファイルテーブルの解析  
	/// セルで罫線を表現するというわけのわからない仕組みのため偶数行だけ取るようにしてる　　
	/// 奇数列は項目の名前("学籍番号"とか)
	pub fn parse_profile(&mut self) -> GenericResult<StudentProfile>
	{
		let rctx = Some(self.main_frame_context());
		let q = jsq::Document.query_selector_all("#TableProfile tr:nth-child(2n) td:nth-child(2n)".into())
			.map_auto("x", jsq::CustomExpression::<jsq::types::String>("x.textContent.trim()".into(), PhantomData))
			.map_value_auto("data", jsqGenObject!{
				id: "data[0]", name: "data[1]", course: "data[2]", grade: "data[3]", semester: "data[4]", address: "data.slice(5, data.length)"
			}).stringify();
		let q: String = self.remote.query_value(rctx, &q.to_string())?.assume();
		Ok(serde_json::from_str(&q).expect("Protocol Corruption"))
	}
	/// 履修テーブルの取得
	/// ## †履修テーブルの仕組み†
	/// - 科目名が入るところは全部rishu-tbl-cellクラスっぽい(科目が入ってるところはbackground-colorスタイルが指定されて白くなっている)
	/// - 科目があるセルはなんと3重table構造(はじめて見た)
	///   - 外側のtableは周囲に1pxの空きをつくるためのもの？
	///   - 2番目のtableが実際のコンテンツレイアウト
	///   - 3番目のtableは科目の詳細(2番目のtableにまとめられそうだけど)
	///   - ちなみに2番目の科目名と3番目は別の行に見えて同一のtd(tr)内(なぜ)
	///   - 空のセルにも1番目のtableだけ入ってる(自動生成の都合っぽい感じ)
	///     - これのおかげで若干空きセルに立体感が出る（？
	pub fn parse_course_table(&mut self) -> GenericResult<CourseTable>
	{
		let rctx = Some(self.main_frame_context());
		let take_link_str = jsqCustomExpr!([jsq::types::Element] "k").query_selector("a".into())
			.map_value_auto("title_link", jsqCustomExpr!([jsq::types::String] "(!title_link) ? null : title_link.textContent.trim()"))
			.into_closure("k");
		let q = jsq::Document.query_selector_all("table.rishu-tbl-cell".into())
			.map_value_auto("tables", jsqCustomExpr!([jsq::types::Array<jsq::types::Element>] "[tables[3], tables[5]]"))
			.map_auto("koma", jsqCustomExpr!([jsq::types::Element] "koma").query_selector_all("td.rishu-tbl-cell".into()).map(take_link_str));
		self.remote.query_value(rctx, &format!(r#"
			let komas = {};
			var first_quarter = [], last_quarter = [];
			for(var i = 0; i < komas[0].length; i += 6)
			{{
				first_quarter.push({{
					monday:   komas[0][i + 0], tuesday: komas[0][i + 1], wednesday: komas[0][i + 2],
					thursday: komas[0][i + 3], friday:  komas[0][i + 4], saturday:  komas[0][i + 5]
				}});
				last_quarter.push({{
					monday:   komas[1][i + 0], tuesday: komas[1][i + 1], wednesday: komas[1][i + 2],
					thursday: komas[1][i + 3], friday:  komas[1][i + 4], saturday:  komas[1][i + 5]
				}});
			}}
			JSON.stringify({{ firstQuarter: first_quarter, lastQuarter: last_quarter }})
		"#, q)).and_then(|s| serde_json::from_str(&s.assume_string()).map_err(From::from))
	}
	/// 卒業要件集計欄のデータを取得
	pub fn parse_graduation_requirements_table(&mut self) -> GenericResult<GraduationRequirements>
	{
		let rctx = Some(self.main_frame_context());
		let query_text_content = jsqCustomExpr!([jsq::types::String] "x.textContent.trim()").into_closure("x");
		self.remote.query_value(rctx, &jsq::Document.query_selector_all("#dgrdSotsugyoYoken tr.text-main td:not(:first-child)".into()).map(query_text_content)
			.map_value_auto("cells", jsqGenObject!{
				requirements: &jsqGenObject!{
					intercom: "parseInt(cells[0])", selfdev:  "parseInt(cells[1])", general:  "parseInt(cells[2])",
					basic:    "parseInt(cells[3])", practice: "parseInt(cells[4])", research: "parseInt(cells[5])",
					totalRequired: "parseInt(cells[6])", totalSelected: "parseInt(cells[7])", total: "0"
				}.to_string(),
				mastered: &jsqGenObject!{
					intercom: "parseInt(cells[9 + 0])", selfdev:  "parseInt(cells[9 + 1])", general:  "parseInt(cells[9 + 2])",
					basic:    "parseInt(cells[9 + 3])", practice: "parseInt(cells[9 + 4])", research: "parseInt(cells[9 + 5])",
					totalRequired: "parseInt(cells[9 + 6])", totalSelected: "parseInt(cells[9 + 7])", total: "parseInt(cells[9 + 8])"
				}.to_string(),
				current: &jsqGenObject!{
					intercom: "parseInt(cells[18 + 0])", selfdev:  "parseInt(cells[18 + 1])", general:  "parseInt(cells[18 + 2])",
					basic:    "parseInt(cells[18 + 3])", practice: "parseInt(cells[18 + 4])", research: "parseInt(cells[18 + 5])",
					totalRequired: "parseInt(cells[18 + 6])", totalSelected: "parseInt(cells[18 + 7])", total: "parseInt(cells[18 + 8])"
				}.to_string()
			}).stringify().to_string()).and_then(|x| serde_json::from_str(&x.assume_string()).map_err(From::from))
	}
}
/// 学生プロファイル
#[derive(Serialize, Deserialize, Debug, Clone)] #[serde(rename_all = "camelCase")]
pub struct StudentProfile
{
	#[doc = "学籍番号"] pub id: String,
	#[doc = "名前"] pub name: String,
	#[doc = "学部"] pub course: String,
	#[doc = "学年(年込み)"] pub grade: String,
	#[doc = "セメスタ(よくわからん)"] pub semester: String,
	#[doc = "住所"] pub address: Vec<String>
}
/// 履修科目テーブル
#[derive(Serialize, Deserialize, Debug, Clone)] #[serde(rename_all = "camelCase")]
pub struct CourseTable
{
	#[doc = "前半クォーター"] pub first_quarter: Vec<WeeklyCourse>,
	#[doc = "後半クォーター"]  pub last_quarter: Vec<WeeklyCourse>
}
/// 履修科目 曜日別
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WeeklyCourse
{
	pub monday: Option<String>, pub tuesday: Option<String>, pub wednesday: Option<String>,
	pub thursday: Option<String>, pub friday: Option<String>, pub saturday: Option<String>
}
/// 卒業要件集計テーブル
#[derive(Serialize, Deserialize, Debug, Clone)] #[serde(rename_all = "camelCase")] #[repr(C)]
pub struct GraduationRequirements
{
	#[doc = "必要単位数"] pub requirements: CategorizedUnits,
	#[doc = "習得済単位"] pub mastered: CategorizedUnits,
	#[doc = "履修中単位"] pub current: CategorizedUnits
}
/// 講義カテゴリ別単位数
#[derive(Serialize, Deserialize, Debug, Clone)] #[serde(rename_all = "camelCase")] #[repr(C)]
pub struct CategorizedUnits
{
	#[doc = "国際コミュニケーション"] pub intercom: u16,
	#[doc = "セルフディベロップメント"] pub selfdev: u16,
	#[doc = "一般教養"] pub general: u16,
	#[doc = "専門基礎"] pub basic: u16,
	#[doc = "専門デジタル演習"] pub practice: u16,
	#[doc = "研究"] pub research: u16,
	// 下２つはクライアントサイドで計算し直してもいいかも
	// (CampusPlanのほうの計算は必要単位数を加味していないので若干正確ではない)
	#[doc = "必修小計"] pub total_required: u16,
	#[doc = "選択小計"] pub total_selected: u16,
	#[doc = "総計"] pub total: u16
}

/// 出欠状況参照ページ
pub enum AttendanceDetailsPage { }
impl CampusPlanAttendanceDetailsFrames
{
	const TABLE_ID: &'static str = "dg";
	const BY_PERIOD_TABLE_ID: &'static str = "dgKikanbetsu";
	const COMMONCODE: &'static str = r#"function toPeriod(s) {
		switch(s) {
		case "1 Q": return "FirstQuarter"; case "2 Q": return "SecondQuarter";
		case "3 Q": return "ThirdQuarter";  case "4 Q": return "FourthQuarter";
		case "前期": return "FirstStage"; case "後期": return "LateStage";
		case "通年": return "WholeYear"; default: console.assert(false);
		}
	}"#;
	
	/// 今年度の出欠状況テーブルを取得
	pub fn parse_current_year_table(&mut self) -> GenericResult<Vec<SubjectAttendanceState>>
	{
		let rctx = Some(self.main_frame_context());
		let cells = jsq::Document.query_selector_all(format!("#{} tr:not(:first-child) td", Self::TABLE_ID))
			.map_auto("x", jsqCustomExpr!([jsq::types::String] "x.textContent.trim()"));
		let objgen = jsqGenObject!{
			code: "cells[i + 0]", name: "cells[i + 1]", period: "toPeriod(cells[i + 2])", week: "toWeekName(cells[i + 3])",
			// 半角にしてからparseInt
			time: "parseInt(cells[i + 4].replace(/[０-９]/g, x => String.fromCharCode(x.charCodeAt(0) - 65248)))",
			rate: "parseFloat(cells[i + 5])", states: r#"cells.slice(i + 6, i + 6 + 15).map(x =>
			{
				if(!x) return [0, 0, "NoData"];
				var date = x.match(/(\d+)\/(\d+)/);
				if(x.includes("公認欠席"))  return [parseInt(date[1]), parseInt(date[2]), "Authorized"];
				else if(x.includes("欠席")) return [parseInt(date[1]), parseInt(date[2]), "Absence"];
				else if(x.includes("出席")) return [parseInt(date[1]), parseInt(date[2]), "Presence"];
				else return [parseInt(date[1]), parseInt(date[2]), "NoData"];
			})"#
		};
		self.remote.query_value(rctx, &format!(r#"
			{}
			function toWeekName(s)
			{{
				switch(s)
				{{
				case "月曜日": return "Monday"; case "火曜日": return "Tuesday"; case "水曜日": return "Wednesday";
				case "木曜日": return "Thursday"; case "金曜日": return "Friday"; case "土曜日": return "Saturday";
				default: console.assert(false);
				}}
			}}

			let cells = {};
			var subjects = [];
			for(var i = 0; i < cells.length; i += 15 + 6) subjects.push({});
			JSON.stringify(subjects)
		"#, Self::COMMONCODE, cells, objgen)).and_then(|s| serde_json::from_str(&s.assume_string()).map_err(From::from))
	}
	/// 期間別出席率テーブルの取得
	pub fn parse_attendance_rates(&mut self) -> GenericResult<Vec<PeriodAttendanceRate>>
	{
		let rctx = Some(self.main_frame_context());
		let q_cells = jsq::Document.query_selector_all(format!("#{} tr:not(:first-child) td", Self::BY_PERIOD_TABLE_ID))
			.map_auto("x", jsqCustomExpr!([jsq::types::String] "x.textContent.trim()"));
		let q_objcon = jsqGenObject!{ firstYear: "parseInt(row[0])", startingPeriod: "toPeriod(row[1])", rates: "parseFloat(row[2])" }
			.into_closure("row");
		let q: String = self.remote.query_value(rctx, &format!(r#"{}
			let cells2 = {}; var ret = [];
			for(var i = 0; i < cells2.length; i += 3) ret.push(({})(cells2.slice(i, i + 3)));
			JSON.stringify(ret)
		"#, Self::COMMONCODE, q_cells, q_objcon))?.assume();
		Ok(serde_json::from_str(&q).expect("Protocol Corruption"))
	}
}
/// 出欠テーブル: 科目行
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)] #[serde(rename_all = "camelCase")]
pub struct SubjectAttendanceState
{
	#[doc = "講義コード"] pub code: String,
	#[doc = "講義名称"] pub name: String,
	#[doc = "開講時期"] pub period: Period,
	#[doc = "代表曜日"] pub week: Week,
	#[doc = "代表時限"] pub time: u32,
	#[doc = "出席率"] pub rate: f32,
	#[doc = "日ごとの出欠状態"] pub states: Vec<(u32, u32, AttendanceState)>
}
/// 出席率テーブル
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)] #[serde(rename_all = "camelCase")]
pub struct PeriodAttendanceRate
{
	#[doc = "初年度"] pub first_year: u32,
	#[doc = "開始時期"] pub starting_period: Period,
	#[doc = "出席率"] pub rates: f32
}
/// 開講時期
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period
{
	#[doc = "通年"] WholeYear,
	#[doc = "前期"] FirstStage, #[doc = "後期"] LateStage,
	#[doc = "1Q"] FirstQuarter, #[doc = "2Q"] SecondQuarter, #[doc = "3Q"] ThirdQuarter, #[doc = "4Q"] FourthQuarter
}
/// 曜日
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Week
{
	Monday, Tuesday, Wednesday, Thursday, Friday, Saturday
}
/// 出席状態
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttendanceState
{
	#[doc = "データなし"] NoData,
	#[doc = "出席"] Presence, #[doc = "欠席"] Absence, #[doc = "公認欠席"] Authorized
}
