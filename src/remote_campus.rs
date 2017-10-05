//! DigitalCampus Remote Controllers

#![allow(dead_code)]

use {headless_chrome, GenericResult};
use headless_chrome::{SessionEventSubscriber, SessionEventSubscribable, Event, RequestID};
use std::net::TcpStream;
use serde_json;
use serde_json::{Value as JValue, Map as JMap};
use std::marker::PhantomData;
use std::mem::{replace, transmute};

use headless_chrome::runtime;
// use headless_chrome::runtime::JSONTyping;

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
		unsafe
		{
			let objptr: *mut RemoteCampus = &object as *const _ as *mut _;
			(&mut object.session as &mut SessionEventSubscribable<headless_chrome::page::FrameNavigated>).subscribe_session_event_raw(objptr);
		}
		object.session.page().enable(0).unwrap(); object.session.wait_result(0).unwrap();
		object.session.dom().enable(0).unwrap(); object.session.wait_result(0).unwrap();
		object.session.runtime().enable(0).unwrap(); object.session.wait_result(0).unwrap();
		Ok(object)
	}
	fn new_request_id(&mut self) -> RequestID
	{
		let r = self.request_id; self.request_id += 1; r
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
	pub fn query_page_location(&mut self) -> GenericResult<String>
	{
		self.query_value(None, "location.href").map(runtime::RemoteObject::assume_string)
	}
	pub fn query_page_location_in(&mut self, cid: u64) -> GenericResult<String>
	{
		self.query_value(Some(cid), "location.href").map(runtime::RemoteObject::assume_string)
	}
	pub fn is_in_login_page(&mut self) -> GenericResult<bool>
	{
		Ok(self.query_page_location()?.contains("/campuslogin"))
	}
	pub fn is_in_home(&mut self) -> GenericResult<bool>
	{
		Ok(self.query_page_location()?.contains("/campusHomepage"))
	}

	pub fn click_element(&mut self, context: Option<u64>, selector: &str) -> GenericResult<&mut Self>
	{
		self.query(context, &format!(r#"document.querySelector({:?}).click()"#, selector)).map(move |_| self)
	}
	pub fn jump_to_anchor_href(&mut self, selector: &str) -> GenericResult<&mut Self>
	{
		let id = self.new_request_id(); let id2 = self.new_request_id();
		let intersys_link_attrs = self.session.dom().get_root_node_sync(id)?.query_selector(selector)?.attributes()?;
		let href_index = intersys_link_attrs.iter().position(|s| s == "href").unwrap() + 1;
		self.session.page().navigate_sync(id2, intersys_link_attrs[href_index].as_str().unwrap()).map(move |_| self)
	}

	/// synchronize page
	pub fn wait_loading(&mut self) -> GenericResult<&mut Self>
	{
		self.session.wait_event::<headless_chrome::page::LoadEventFired>().map(move |_| self)
	}
}
impl SessionEventSubscriber<headless_chrome::page::FrameNavigated> for RemoteCampus
{
	fn on_event(&mut self, event: &headless_chrome::page::FrameNavigated)
	{
		if let Some(n) = event.name.as_ref()
		{
			println!("FrameNavigated: {} in {}", event.url, n);
		}
		else
		{
			println!("FrameNavigated: {}", event.url);
		}
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
impl HomePage
{
	const INTERSYS_LINK_PATH: &'static str = "#gnav ul li.menuBlock ul li:first-child a";
	/// "履修・成績・出席"リンクへ
	/// 将来的にmenuBlockクラスが複数出てきたらまた考えます
	pub fn access_intersys(mut self) -> GenericResult<CampusPlanEntryFrames>
	{
		self.remote.jump_to_anchor_href(Self::INTERSYS_LINK_PATH)?;
		let mut r = CampusPlanFrames::enter(self.remote);
		r.wait_frame_context(true)?; Ok(r)
	}
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
			if ($x)($($ee::)+deserialize($params)).require_break() { break; }
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
			if ($x)($($ee::)+deserialize($params)).require_break() { break; }
		}
		else { SessionEventLoop!{ __SessionMatcher($name, $params) $($rest)* } }
	};
	($session: expr; { $($content: tt)* }) =>
	{
		loop
		{
			let s = $session.wait_text()?;
			#[cfg(feature = "verbose")] println!("[SessionEventLoop]Received: {:?}", s);
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
	fn enter(remote: RemoteCampus) -> Self
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
		self.remote.query_page_location_in(cid).map(|l| l.contains("/blank.html"))
	}
}

/// Context ops
impl<MainFrameCtrlTy: PageControl, MenuFrameCtrlTy: PageControl> CampusPlanFrames<MainFrameCtrlTy, MenuFrameCtrlTy>
{
	/// フレームのロードを待つ
	fn wait_frame_context(&mut self, wait_for_menu_context: bool) -> GenericResult<&mut Self>
	{
		let (mut main_completion, mut menu_completion) = (false, !wait_for_menu_context);

		SessionEventLoop!(self.remote.session;
		{
			headless_chrome::page::FrameNavigated => |e: headless_chrome::page::FrameNavigated|
			{
				self.remote.session.dispatch_frame_navigated(&e);
				match e.name.as_ref().map(|s| s as &str)
				{
					Some("MainFrame") => { self.ctx_main_frame.navigated(e.frame_id); },
					Some("MenuFrame") => { self.ctx_menu_frame.navigated(e.frame_id); },
					_ => ()
				}
			};
			headless_chrome::runtime::ExecutionContextCreated => |e: headless_chrome::runtime::ExecutionContextCreated|
			{
				if let Some(fid) = e.aux.get("frameId").and_then(JValue::as_str)
				{
					self.ctx_main_frame.try_attach_context(fid, e.context_id);
					self.ctx_menu_frame.try_attach_context(fid, e.context_id);
				}
			};
			headless_chrome::runtime::ExecutionContextDestroyed => |e: headless_chrome::runtime::ExecutionContextDestroyed|
			{
				if Some(e.context_id) == self.ctx_main_frame.contextid() { self.ctx_main_frame.detach_context(); }
				if Some(e.context_id) == self.ctx_menu_frame.contextid() { self.ctx_menu_frame.detach_context(); }
			};
			headless_chrome::runtime::ExecutionContextsCleared => |_|
			{
				self.ctx_main_frame.detach_context();
				self.ctx_menu_frame.detach_context();
			};
			headless_chrome::page::FrameStoppedLoading => |e: headless_chrome::page::FrameStoppedLoading|
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
		let mut r = CampusPlanFrames::enter(self.remote);
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
		let mut r = CampusPlanFrames::enter(self.remote);
		r.wait_frame_context(true)?;
		while r.is_blank_main()? { r.wait_frame_context(true)?; }
		Ok(r)
	}
	/// 出欠関係セクションへ
	pub fn access_attendance_category(mut self) -> GenericResult<CampusPlanAttendanceFrames>
	{
		let rctx = Some(self.main_frame_context());
		self.remote.click_element(rctx, Self::ATTENDANCE_CATEGORY_LINK_ID)?;
		let mut r = CampusPlanFrames::enter(self.remote);
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
		self.remote.query_value(rctx, r#"
			var data = Array.prototype.map.call(document.querySelectorAll('#TableProfile tr:nth-child(2n) td:nth-child(2n)'), x => x.textContent.trim());
			JSON.stringify({
				id: data[0], name: data[1], course: data[2], grade: data[3], semester: data[4], address: data.slice(5, data.length)
			})
		"#).and_then(|s| serde_json::from_str(&s.assume_string()).map_err(From::from))
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
		self.remote.query_value(rctx, r#"
			var tables = document.querySelectorAll('table.rishu-tbl-cell');
			// 前半クォーターは3、後半クォーターは5
			var q1_koma_cells = tables[3].querySelectorAll('td.rishu-tbl-cell');
			var q2_koma_cells = tables[5].querySelectorAll('td.rishu-tbl-cell');
			var first_koma_cells = Array.prototype.map.call(q1_koma_cells, function(k)
			{
				var title_link = k.querySelector('a');
				if(!title_link) return null; else return title_link.textContent.trim();
			});
			var last_koma_cells = Array.prototype.map.call(q2_koma_cells, function(k)
			{
				var title_link = k.querySelector('a');
				if(!title_link) return null; else return title_link.textContent.trim();
			});
			var first_quarter = [], last_quarter = [];
			for(var i = 0; i < first_koma_cells.length; i += 6)
			{
				first_quarter.push({
					monday: first_koma_cells[i + 0], tuesday: first_koma_cells[i + 1], wednesday: first_koma_cells[i + 2],
					thursday: first_koma_cells[i + 3], friday: first_koma_cells[i + 4], saturday: first_koma_cells[i + 5]
				});
				last_quarter.push({
					monday: last_koma_cells[i + 0], tuesday: last_koma_cells[i + 1], wednesday: last_koma_cells[i + 2],
					thursday: last_koma_cells[i + 3], friday: last_koma_cells[i + 4], saturday: last_koma_cells[i + 5]
				});
			}
			JSON.stringify({ firstQuarter: first_quarter, lastQuarter: last_quarter })
		"#).and_then(|s| serde_json::from_str(&s.assume_string()).map_err(From::from))
	}
	/// 卒業要件集計欄のデータを取得
	pub fn parse_graduation_requirements_table(&mut self) -> GenericResult<GraduationRequirements>
	{
		let rctx = Some(self.main_frame_context());
		self.remote.query_value(rctx, r#"
			var table = document.getElementById('dgrdSotsugyoYoken');
			var cells = Array.prototype.map.call(table.querySelectorAll('tr.text-main td:not(:first-child)'), x => x.textContent.trim());
			JSON.stringify({
				requirements: {
					intercom: parseInt(cells[0]), selfdev:  parseInt(cells[1]), general:  parseInt(cells[2]),
					basic:    parseInt(cells[3]), practice: parseInt(cells[4]), research: parseInt(cells[5]),
					totalRequired: parseInt(cells[6]), totalSelected: parseInt(cells[7]), total: 0
				},
				mastered: {
					intercom: parseInt(cells[9 + 0]), selfdev:  parseInt(cells[9 + 1]), general:  parseInt(cells[9 + 2]),
					basic:    parseInt(cells[9 + 3]), practice: parseInt(cells[9 + 4]), research: parseInt(cells[9 + 5]),
					totalRequired: parseInt(cells[9 + 6]), totalSelected: parseInt(cells[9 + 7]), total: parseInt(cells[9 + 8])
				},
				current: {
					intercom: parseInt(cells[18 + 0]), selfdev:  parseInt(cells[18 + 1]), general:  parseInt(cells[18 + 2]),
					basic:    parseInt(cells[18 + 3]), practice: parseInt(cells[18 + 4]), research: parseInt(cells[18 + 5]),
					totalRequired: parseInt(cells[18 + 6]), totalSelected: parseInt(cells[18 + 7]), total: parseInt(cells[18 + 8])
				}
			})
		"#).and_then(|x| serde_json::from_str(&x.assume_string()).map_err(From::from))
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

			var table = document.getElementById({:?});
			var cells = Array.prototype.map.call(table.querySelectorAll("tr:not(:first-child) td"), x => x.textContent.trim());
			var subjects = [];
			for(var i = 0; i < cells.length; i += 15 + 6)
			{{
				subjects.push({{
					code: cells[i + 0], name: cells[i + 1], period: toPeriod(cells[i + 2]), week: toWeekName(cells[i + 3]),
					// 半角にしてからparseInt
					time: parseInt(cells[i + 4].substring(cells[i + 4].search(/\d+/)).replace(/[０-ｚ]/g,
						x => String.fromCharCode(x.charCodeAt(0) - 65248))),
					rate: parseFloat(cells[i + 5].substring(cells[i + 5].search(/\d+(\.\d+)?/))),
					attendanceCells: cells.slice(i + 6, i + 6 + 15).map(x =>
					{{
						if(!x) return [0, 0, "NoData"];
						var date = x.match(/(\d+)\/(\d+)/);
						if(x.includes("公認欠席"))  return [parseInt(date[1]), parseInt(date[2]), "Authorized"];
						else if(x.includes("欠席")) return [parseInt(date[1]), parseInt(date[2]), "Absence"];
						else if(x.includes("出席")) return [parseInt(date[1]), parseInt(date[2]), "Presence"];
						else return [parseInt(date[1]), parseInt(date[2]), "NoData"];
					}})
				}});
			}}
			JSON.stringify(subjects)
		"#, Self::COMMONCODE, Self::TABLE_ID)).and_then(|s| serde_json::from_str(&s.assume_string()).map_err(From::from))
	}
	/// 期間別出席率テーブルの取得
	pub fn parse_attendance_rates(&mut self) -> GenericResult<Vec<PeriodAttendanceRate>>
	{
		let rctx = Some(self.main_frame_context());
		self.remote.query_value(rctx, &format!(r#"
			{}
			var table = document.getElementById({:?});
			var cells = Array.prototype.map.call(table.querySelectorAll("tr:not(:first-child) td"), x => x.textContent.trim());
			var ret = [];
			for(var i = 0; i < cells.length; i += 3) {{
				var row = cells.slice(i, i + 3);
				ret.push({{firstYear: parseInt(row[0]), startingPeriod: toPeriod(row[1]), rates: parseFloat(row[2].substring(row[2].search(/\d+(\.\d+)?/)))}});
			}}
			JSON.stringify(ret)
		"#, Self::COMMONCODE, Self::BY_PERIOD_TABLE_ID)).and_then(|x| serde_json::from_str(&x.assume_string()).map_err(From::from))
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
	#[doc = "セルデータ"] pub attendance_cells: Vec<(u32, u32, DayAttendanceState)>
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
pub enum DayAttendanceState
{
	#[doc = "データなし"] NoData,
	#[doc = "出席"] Presence, #[doc = "欠席"] Absence, #[doc = "公認欠席"] Authorized
}
