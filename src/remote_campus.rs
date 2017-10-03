//! DigitalCampus Remote Controllers

use {headless_chrome, GenericResult};
use headless_chrome::{SessionEventSubscriber, SessionEventSubscribable};
use std::sync::atomic::{Ordering, AtomicUsize};
use std::net::TcpStream;
use serde_json;
use serde_json::{Value as JValue, Map as JMap};
use regex::{Regex, Captures};
use std::marker::PhantomData;
use std::mem::{replace, transmute_copy, transmute};
use std::str::FromStr;

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

/// JavaScriptクエリの結果("result"の中身)
pub struct QueryResult(JMap<String, JValue>);
impl QueryResult
{
	pub fn value(&self) -> &JValue { &self.0["value"] }
	pub fn strip_value<T>(&mut self) -> T where JValue: QueryValueType<T>
	{
		self.0.remove("value").unwrap().unwrap()
	}
}

pub struct RemoteCampus { session: headless_chrome::Session<TcpStream, TcpStream>, request_id: AtomicUsize }
impl RemoteCampus
{
	pub fn connect(addr: &str) -> GenericResult<Self>
	{
		let mut object = headless_chrome::Session::connect(addr).map(|session| RemoteCampus { session, request_id: AtomicUsize::new(1) })?;
		unsafe
		{
			let objptr: *mut RemoteCampus = &object as *const _ as *mut _;
			(&mut object.session as &mut SessionEventSubscribable<headless_chrome::page::FrameNavigated>).subscribe_session_event_raw(objptr);
			(&mut object.session as &mut SessionEventSubscribable<headless_chrome::runtime::ExecutionContextCreated>).subscribe_session_event_raw(objptr);
			(&mut object.session as &mut SessionEventSubscribable<headless_chrome::runtime::ExecutionContextDestroyed>).subscribe_session_event_raw(objptr);
			(&mut object.session as &mut SessionEventSubscribable<headless_chrome::runtime::ExecutionContextsCleared>).subscribe_session_event_raw(objptr);
		}
		object.session.page().enable(0).unwrap(); object.session.wait_result(0).unwrap();
		object.session.dom().enable(0).unwrap(); object.session.wait_result(0).unwrap();
		object.session.runtime().enable(0).unwrap(); object.session.wait_result(0).unwrap();
		Ok(object)
	}
	fn new_request_id(&self) -> usize { self.request_id.fetch_add(1, Ordering::SeqCst) }

	pub fn query(&mut self, context: Option<u64>, expression: &str) -> GenericResult<()>
	{
		let id = self.new_request_id();
		let mut q: JMap<_, _> = if let Some(cid) = context
		{
			self.session.runtime().evaluate_in_sync(id, cid, expression).map(QueryValueType::unwrap)?
		}
		else
		{
			self.session.runtime().evaluate_sync(id, expression).map(QueryValueType::unwrap)?
		};
		let mut qres: JMap<_, _> = q.remove("result").unwrap().unwrap();
		if qres.get("subtype").and_then(JValue::as_str) == Some("error")
		{
			// Error occured
			Err(format!("{:?}", qres).into())
		}
		else { Ok(()) }
	}
	pub fn query_value(&mut self, context: Option<u64>, expression: &str) -> GenericResult<QueryResult>
	{
		let id = self.new_request_id();
		if let Some(cid) = context
		{
			self.session.runtime().evaluate_value_in_sync(id, cid, expression).map(QueryValueType::<JMap<_, _>>::unwrap)
				.map(|mut r| QueryResult(r.remove("result").unwrap().unwrap()))
		}
		else
		{
			self.session.runtime().evaluate_value_sync(id, expression).map(QueryValueType::<JMap<_, _>>::unwrap)
				.map(|mut r| QueryResult(r.remove("result").unwrap().unwrap()))
		}
	}
	pub fn query_page_location(&mut self) -> GenericResult<String>
	{
		self.query_value(None, "location.href").map(|mut v| v.strip_value())
	}
	pub fn is_in_login_page(&mut self) -> GenericResult<bool>
	{
		self.query_page_location().map(|l| l.contains("/campuslogin"))
	}
	pub fn is_in_home(&mut self) -> GenericResult<bool>
	{
		self.query_page_location().map(|l| l.contains("/campusHomepage"))
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
	pub fn sync_load(&mut self, new_location: &str) -> GenericResult<&mut Self>
	{
		let id = self.new_request_id();
		self.session.page().navigate_sync(id, new_location)?; self.sync()
	}

	/// synchronize page
	pub fn sync(&mut self) -> GenericResult<&mut Self>
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
impl SessionEventSubscriber<headless_chrome::runtime::ExecutionContextCreated> for RemoteCampus
{
	fn on_event(&mut self, event: &headless_chrome::runtime::ExecutionContextCreated)
	{
		if let Some(fid) = event.aux.get("frameId").and_then(JValue::as_str)
		{
			println!("ExecutionContext Created for frame {}: {}", fid, event.context_id);
		}
	}
}
impl SessionEventSubscriber<headless_chrome::runtime::ExecutionContextDestroyed> for RemoteCampus
{
	fn on_event(&mut self, event: &headless_chrome::runtime::ExecutionContextDestroyed)
	{
		println!("ExecutionContext Destroyed: {}", event.context_id);
	}
}
impl SessionEventSubscriber<headless_chrome::runtime::ExecutionContextsCleared> for RemoteCampus
{
	fn on_event(&mut self, _: &headless_chrome::runtime::ExecutionContextsCleared)
	{
		println!("ExecutionContexts were cleared by remote browser");
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
		self.remote.sync()?;
		self.remote.wait_login_completion()
	}
}

/// メインページ
pub struct HomePage { remote: RemoteCampus }
impl RemoteCampus
{
	pub unsafe fn assume_home(self) -> HomePage { HomePage { remote: self } }
	pub fn wait_login_completion(mut self) -> GenericResult<Result<HomePage, LoginPage>>
	{
		if self.is_in_home()? { Ok(Ok(unsafe { self.assume_home() })) }
		else if self.is_in_login_page()? { Ok(Err(unsafe { self.assume_login() })) }
		else { self.sync()?; self.wait_login_completion() }
	}
}
impl HomePage
{
	const INTERSYS_LINK_PATH: &'static str = "#gnav ul li.menuBlock ul li:first-child a";
	/// "履修・成績・出席"リンクを処理
	/// 将来的にmenuBlockクラスが複数出てきたらまた考えます
	pub fn jump_into_intersys(mut self) -> GenericResult<Box<CampusPlanEntryFrames>>
	{
		self.remote.jump_to_anchor_href(Self::INTERSYS_LINK_PATH)?;
		let mut r = CampusPlanFrames::enter(self.remote);
		r.remote.sync()?;
		Ok(r)
	}
}

#[derive(Debug)]
pub enum ScriptContextHolder { Unloaded, Empty(String), Context(String, u64) }

/// CampusPlan フレームページ
pub struct CampusPlanFrames<MainFrameCtrlTy: PageControl + 'static>
{
	remote: RemoteCampus, ph: PhantomData<MainFrameCtrlTy>,
	ctx_main_frame: ScriptContextHolder
}
impl<MainFrameCtrlTy: PageControl + 'static> CampusPlanFrames<MainFrameCtrlTy>
{
	fn enter(remote: RemoteCampus) -> Box<Self>
	{
		let mut p = box CampusPlanFrames { remote, ph: PhantomData, ctx_main_frame: ScriptContextHolder::Unloaded };
		unsafe
		{
			let objptr: *mut CampusPlanFrames<MainFrameCtrlTy> = &*p as *const _ as *mut _;
			(&mut p.remote.session as &mut SessionEventSubscribable<headless_chrome::page::FrameNavigated>).subscribe_session_event_raw(objptr);
			(&mut p.remote.session as &mut SessionEventSubscribable<headless_chrome::runtime::ExecutionContextCreated>).subscribe_session_event_raw(objptr);
			(&mut p.remote.session as &mut SessionEventSubscribable<headless_chrome::runtime::ExecutionContextDestroyed>).subscribe_session_event_raw(objptr);
			(&mut p.remote.session as &mut SessionEventSubscribable<headless_chrome::runtime::ExecutionContextsCleared>).subscribe_session_event_raw(objptr);
		} p
	}
	fn leave(&mut self)
	{
		unsafe
		{
			let objptr: *mut CampusPlanFrames<MainFrameCtrlTy> = self as *mut _;
			(&mut self.remote.session as &mut SessionEventSubscribable<headless_chrome::page::FrameNavigated>).unsubscribe_session_event_raw(objptr);
			(&mut self.remote.session as &mut SessionEventSubscribable<headless_chrome::runtime::ExecutionContextCreated>).unsubscribe_session_event_raw(objptr);
			(&mut self.remote.session as &mut SessionEventSubscribable<headless_chrome::runtime::ExecutionContextDestroyed>).unsubscribe_session_event_raw(objptr);
			(&mut self.remote.session as &mut SessionEventSubscribable<headless_chrome::runtime::ExecutionContextsCleared>).unsubscribe_session_event_raw(objptr);
		}
	}
	fn continue_enter<NewMainFrameCtrlTy: PageControl>(self) -> CampusPlanFrames<NewMainFrameCtrlTy>
	{
		unsafe { transmute(self) }
	}
	fn main_frame_context(&self) -> u64
	{
		if let ScriptContextHolder::Context(_, s) = self.ctx_main_frame { s }
		else { panic!("ExecutionContext for MainFrame has not been created yet"); }
	}
	fn wait_main_frame_context(&self) -> u64
	{
		if let ScriptContextHolder::Context(_, s) = self.ctx_main_frame { s }
		else { self.wait_main_frame_context() }
	}
}
impl<MainFrameCtrlTy: PageControl> SessionEventSubscriber<headless_chrome::page::FrameNavigated>
	for CampusPlanFrames<MainFrameCtrlTy>
{
	fn on_event(&mut self, event: &headless_chrome::page::FrameNavigated)
	{
		if event.name.as_ref().map(|x| x as &str) == Some("MainFrame")
		{
			self.ctx_main_frame = ScriptContextHolder::Empty(event.frame_id.clone());
		}
	}
}
impl<MainFrameCtrlTy: PageControl> SessionEventSubscriber<headless_chrome::runtime::ExecutionContextCreated>
	for CampusPlanFrames<MainFrameCtrlTy>
{
	fn on_event(&mut self, event: &headless_chrome::runtime::ExecutionContextCreated)
	{
		let c = match (event.aux.get("frameId").and_then(JValue::as_str), replace(&mut self.ctx_main_frame, ScriptContextHolder::Unloaded))
		{
			(Some(fid), ScriptContextHolder::Empty(s)) =>
				if fid == s { ScriptContextHolder::Context(s, event.context_id) }
				else { ScriptContextHolder::Empty(s) },
			(Some(fid), ScriptContextHolder::Context(s, c)) =>
				if fid == s { ScriptContextHolder::Context(s, event.context_id) }
				else { ScriptContextHolder::Context(s, c) },
			(_, c) => { c }
		};
		self.ctx_main_frame = c;
	}
}
impl<MainFrameCtrlTy: PageControl> SessionEventSubscriber<headless_chrome::runtime::ExecutionContextDestroyed>
	for CampusPlanFrames<MainFrameCtrlTy>
{
	fn on_event(&mut self, event: &headless_chrome::runtime::ExecutionContextDestroyed)
	{
		self.ctx_main_frame = match replace(&mut self.ctx_main_frame, ScriptContextHolder::Unloaded)
		{
			ScriptContextHolder::Context(fid, cid) =>
				if event.context_id == cid { ScriptContextHolder::Empty(fid) }
				else { ScriptContextHolder::Context(fid, cid) },
			c => c
		};
	}
}
impl<MainFrameCtrlTy: PageControl> SessionEventSubscriber<headless_chrome::runtime::ExecutionContextsCleared>
	for CampusPlanFrames<MainFrameCtrlTy>
{
	fn on_event(&mut self, _: &headless_chrome::runtime::ExecutionContextsCleared)
	{
		self.ctx_main_frame = match replace(&mut self.ctx_main_frame, ScriptContextHolder::Unloaded)
		{
			ScriptContextHolder::Context(fid, _) => ScriptContextHolder::Empty(fid),
			c => c
		};
	}
}
pub type CampusPlanEntryFrames      = CampusPlanFrames<CampusPlanEntryPage>;
pub type CampusPlanCourseFrames     = CampusPlanFrames<CampusPlanCoursePage>;
pub type CampusPlanSyllabusFrames   = CampusPlanFrames<CampusPlanSyllabusPage>;
pub type CampusPlanAttendanceFrames = CampusPlanFrames<CampusPlanAttendancePage>;
/*impl<MainFrameCtrlTy: PageControl + 'static> CampusPlanFrames<MainFrameCtrlTy>
{
	/// ほしいフレームの中身のみ表示してメインコンテキストにする
	pub fn isolate_frame(mut self, name: &str) -> GenericResult<RemoteCampus>
	{
		let id = self.remote.new_request_id();
		let restree = self.remote.session.page().get_resource_tree_sync(id)?;
		let main_frame = restree["frameTree"]["childFrames"].as_array().unwrap().iter().find(|e| e["frame"]["name"] == name).unwrap();
		self.remote.sync_load(main_frame["frame"]["url"].as_str().unwrap())?;
		Ok(self.remote)
	}
	/// ほしいフレームのロードイベントを横取りしてメインコンテキストにする
	pub fn isolate_frame_stealing_load(mut self, name: &str) -> GenericResult<RemoteCampus>
	{
		let frame_nav_begin = self.remote.session.wait_event::<headless_chrome::page::FrameNavigated>().unwrap();
		if frame_nav_begin.name.as_ref().map(|x| x == name).unwrap_or(false)
		{
			self.remote.sync_load(&frame_nav_begin.url)?; Ok(self.remote)
		}
		else { self.isolate_frame_stealing_load(name) }
	}
	/// メインフレームのみ独立
	pub fn isolate_mainframe(self) -> GenericResult<MainFrameCtrlTy>
	{
		self.isolate_frame("MainFrame").map(|x| unsafe { MainFrameCtrlTy::with_remote(x) })
	}
	/// メインフレームのみ独立(イベント奪取による)
	pub fn isolate_mainframe_stealing_load(self) -> GenericResult<MainFrameCtrlTy>
	{
		self.isolate_frame_stealing_load("MainFrame").map(|x| unsafe { MainFrameCtrlTy::with_remote(x) })
	}
}*/

/// Tag(CampusPlanのエントリーページを表す)
pub enum CampusPlanEntryPage {}
/// コンテンツ操作に関わる
impl CampusPlanFrames<CampusPlanEntryPage>
{
	const COURSE_CATEGORY_LINK_ID:     &'static str = "#dgSystem__ctl2_lbtnSystemName";
	#[allow(dead_code)]
	const SYLLABUS_CATEGORY_LINK_ID:   &'static str = "#dgSystem__ctl3_lbtnSystemName";
	const ATTENDANCE_CATEGORY_LINK_ID: &'static str = "#dgSystem__ctl4_lbtnSystemName";

	/// 履修関係セクションへ
	pub fn jump_into_course_category(mut self) -> GenericResult<Box<CampusPlanCourseFrames>>
	{
		let rctx = Some(self.main_frame_context());
		self.remote.click_element(rctx, Self::COURSE_CATEGORY_LINK_ID)?;
		self.leave(); let mut r = CampusPlanFrames::enter(self.remote);
		r.remote.sync()?; r.wait_main_frame_context(); Ok(r)
	}
	/// Webシラバスセクションへ
	#[allow(dead_code)]
	pub fn jump_into_syllabus_category(mut self) -> GenericResult<Box<CampusPlanSyllabusFrames>>
	{
		let rctx = Some(self.main_frame_context());
		self.remote.click_element(rctx, Self::SYLLABUS_CATEGORY_LINK_ID)?;
		self.leave(); let mut r = CampusPlanFrames::enter(self.remote);
		r.remote.sync()?; r.wait_main_frame_context(); Ok(r)
	}
	/// 出欠関係セクションへ
	pub fn jump_into_attendance_category(mut self) -> GenericResult<Box<CampusPlanAttendanceFrames>>
	{
		let rctx = Some(self.main_frame_context());
		self.remote.click_element(rctx, Self::ATTENDANCE_CATEGORY_LINK_ID)?;
		self.leave(); let mut r = CampusPlanFrames::enter(self.remote);
		r.remote.sync()?; r.wait_main_frame_context(); Ok(r)
	}
}
/// Tag(CampusPlanの履修関係メニューページを表す)
pub enum CampusPlanCoursePage { }
impl CampusPlanFrames<CampusPlanCoursePage>
{
	/// 履修チェック結果の確認ページへ
	/// * 履修登録期間中はこれだと動かないかもしれない
	pub fn jump_into_course_details(mut self) -> GenericResult<CampusPlanFrames<CourseDetailsPage>>
	{
		let rctx = Some(self.main_frame_context());
		self.remote.click_element(rctx, "#dgSystem__ctl2_lbtnPage")?.sync()?;
		Ok(self.continue_enter())
	}
}
/// 未実装
#[allow(dead_code)]
pub enum CampusPlanSyllabusPage { }
pub enum CampusPlanAttendancePage { }
impl CampusPlanFrames<CampusPlanAttendancePage>
{
	const ATTENDANCE_REF_LINK_ID: &'static str = "#dgSystem__ctl2_lbtnPage";
	/// 出欠状況参照ページへ
	pub fn jump_into_details(mut self) -> GenericResult<CampusPlanFrames<AttendanceDetailsPage>>
	{
		let rctx = Some(self.main_frame_context());
		self.remote.click_element(rctx, Self::ATTENDANCE_REF_LINK_ID)?.sync()?;
		Ok(self.continue_enter())
	}
}
pub trait PageControl: Sized { }
impl PageControl for CampusPlanEntryPage  { }
impl PageControl for CampusPlanCoursePage { }
impl PageControl for CampusPlanSyllabusPage { }
impl PageControl for CampusPlanAttendancePage { }
impl PageControl for CourseDetailsPage { }
impl PageControl for AttendanceDetailsPage { }

/// 履修確認ページ
pub enum CourseDetailsPage { }
/// 学生プロファイル/履修データの解析周り
impl CampusPlanFrames<CourseDetailsPage>
{
	/// 学生プロファイルテーブルの解析　　
	/// セルで罫線を表現するというわけのわからない仕組みのため偶数行だけ取るようにしてる　　
	/// 奇数列は項目の名前("学籍番号"とか)
	pub fn parse_profile(&mut self) -> GenericResult<StudentProfile>
	{
		let rctx = Some(self.main_frame_context());
		let profile_rows_data = self.remote.query_value(rctx, 
			r#"Array.prototype.map.call(document.querySelectorAll('#TableProfile tr:nth-child(2n) td:nth-child(2n)'), x => x.textContent)"#)?;
		let regex_replace_encoded = Regex::new(r"\\u\{([0-9a-fA-F]{4})\}").unwrap();
		let mut profile_rows: Vec<_> = profile_rows_data.value().as_array().unwrap().iter()
			.map(|v| regex_replace_encoded.replace_all(v.as_str().unwrap().trim(),
				|cap: &Captures| String::from_utf16(&[u16::from_str_radix(&cap[1], 16).unwrap()]).unwrap()).into_owned()
			).collect();

		Ok(StudentProfile
		{
			id: profile_rows.remove(0), name: profile_rows.remove(0),
			course: profile_rows.remove(0), grade: profile_rows.remove(0),
			semester: profile_rows.remove(0), address: profile_rows
		})
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
		// 下のスクリプトで得られるデータは行優先です(0~5が1限、6~11が2限といった感じ)
		let rctx = Some(self.main_frame_context());
		let values: Vec<_> = self.remote.query_value(rctx, r#"
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
		"#)?.strip_value();
		let course_table: Vec<Vec<_>> = values.into_iter().map(|v| QueryValueType::<Vec<_>>::unwrap(v).into_iter().map(|vs| match vs
			{
				serde_json::Value::Null => String::new(),
				serde_json::Value::String(s) => s,
				_ => api_corruption!(value_type)
			}).collect()).collect();

		Ok(CourseTable
		{
			first_quarter: course_table[0].chunks(6).map(ToOwned::to_owned).collect(),
			last_quarter: course_table[1].chunks(6).map(ToOwned::to_owned).collect()
		})
	}
	/// 卒業要件集計欄のデータを取得
	pub fn parse_graduation_requirements_table(&mut self) -> GenericResult<GraduationRequirements>
	{
		let rctx = Some(self.main_frame_context());
		let content_values: Vec<_> = self.remote.query_value(rctx, r#"
			var table = document.getElementById('dgrdSotsugyoYoken');
			var rows = table.querySelectorAll('tr.text-main td:not(:first-child)');
			Array.prototype.map.call(rows, x => x.textContent)
		"#)?.strip_value();
		let mut content = content_values.into_iter().map(|s| QueryValueType::<String>::unwrap(s).parse());

		Ok(GraduationRequirements
		{
			requirements: From::from(content.by_ref().take(8).collect::<Result<Vec<u16>, _>>()?),
			mastered: From::from(content.by_ref().skip(1).take(9).collect::<Result<Vec<u16>, _>>()?),
			current: From::from(content.by_ref().take(9).collect::<Result<Vec<u16>, _>>()?)
		})
	}
}
/// 学生プロファイル
#[derive(Serialize, Deserialize)] #[serde(rename_all = "camelCase")]
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
#[derive(Serialize, Deserialize)] #[serde(rename_all = "camelCase")]
pub struct CourseTable
{
	#[doc = "前半クォーター"] pub first_quarter: Vec<Vec<String>>,
	#[doc = "後半クォーター"]  pub last_quarter: Vec<Vec<String>>
}
/// 卒業要件集計テーブル
#[derive(Serialize, Deserialize)] #[serde(rename_all = "camelCase")] #[repr(C)]
pub struct GraduationRequirements
{
	#[doc = "必要単位数"] pub requirements: CategorizedUnits,
	#[doc = "習得済単位"] pub mastered: CategorizedUnits,
	#[doc = "履修中単位"] pub current: CategorizedUnits
}
/// 講義カテゴリ別単位数
#[derive(Serialize, Deserialize)] #[serde(rename_all = "camelCase")] #[repr(C)]
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
impl From<Vec<u16>> for CategorizedUnits
{
	fn from(mut v: Vec<u16>) -> CategorizedUnits
	{
		if v.len() < 9 { v.resize(9, 0); }
		unsafe { transmute_copy(&*(v.as_ptr() as *const [u16; 9])) }
	}
}

/// 出欠状況参照ページ
pub enum AttendanceDetailsPage { }
impl CampusPlanFrames<AttendanceDetailsPage>
{
	const TABLE_ID: &'static str = "dg";
	const BY_PERIOD_TABLE_ID: &'static str = "dgKikanbetsu";
	
	/// 今年度の出欠状況テーブルを取得
	pub fn parse_current_year_table(&mut self) -> GenericResult<Vec<SubjectAttendanceState>>
	{
		let rctx = Some(self.main_frame_context());
		let mut res_values: Vec<_> = self.remote.query_value(rctx, &format!(r#"
			var table = document.getElementById({:?});
			var rows = table.querySelectorAll("tr:not(:first-child) td");
			Array.prototype.map.call(rows, x => x.textContent)
		"#, Self::TABLE_ID))?.strip_value();

		/// 時限の数値変換(全角なのでparseで取れない)
		fn parse_opening_time(s: &str) -> u32
		{
			     if s == "１" { 1 } else if s == "２" { 2 } else if s == "３" { 3 }
			else if s == "４" { 4 } else if s == "５" { 5 } else if s == "６" { 6 }
			else if s == "７" { 7 } else if s == "８" { 8 } else { 0 }
		}

		let re_nums = Regex::new(r"\d+").unwrap();
		let re_floatings = Regex::new(r"\d+(.\d)?").unwrap();
		let re_date = Regex::new(r"(\d+)/(\d+)").unwrap();
		let mut subjects = Vec::new();
		while !res_values.is_empty()
		{
			subjects.push(SubjectAttendanceState
			{
				code: res_values.remove(0).as_str().unwrap().trim().to_owned(),
				name: res_values.remove(0).as_str().unwrap().trim().to_owned(),
				period: Period::from_str(res_values.remove(0).as_str().unwrap().trim()).unwrap(),
				week: Week::from_str(res_values.remove(0).as_str().unwrap().trim()).unwrap(),
				time: parse_opening_time(re_nums.find(res_values.remove(0).as_str().unwrap().trim()).unwrap().as_str()),
				rate: re_floatings.find(res_values.remove(0).as_str().unwrap().trim()).unwrap().as_str().parse().unwrap(),
				attendance_cells: res_values.drain(..15).map(|s|
				{
					let s = s.as_str().unwrap();
					if s.is_empty() { (0, 0, DayAttendanceState::NoData) }
					else
					{
						let date = re_date.captures(s).unwrap();
						let (m, d) = (date[1].parse().unwrap(), date[2].parse().unwrap());
						(m, d, if s.contains("公認欠席") { DayAttendanceState::Authorized }
						else if s.contains("出席") { DayAttendanceState::Presence }
						else if s.contains("欠席") { DayAttendanceState::Absence }
						else { DayAttendanceState::NoData })
					}
				}).collect()
			})
		}
		Ok(subjects)
	}
	/// 期間別出席率テーブルの取得
	pub fn parse_attendance_rates(&mut self) -> GenericResult<PeriodAttendanceRates>
	{
		let rctx = Some(self.main_frame_context());
		let res_values: Vec<_> = self.remote.query_value(rctx, &format!(r#"
			var table = document.getElementById({:?});
			var rows = table.querySelectorAll("tr:not(:first-child) td");
			Array.prototype.map.call(rows, x => x.textContent)
		"#, Self::BY_PERIOD_TABLE_ID))?.strip_value();

		let re_floatings = Regex::new(r"\d+(.\d)?").unwrap();
		Ok(PeriodAttendanceRates
		{
			first_year: res_values[0].as_str().unwrap().trim().parse().unwrap(),
			starting_period: Period::from_str(res_values[1].as_str().unwrap().trim()).unwrap(),
			rates: res_values.into_iter().skip(2).step_by(3).map(|rs| re_floatings.find(rs.as_str().unwrap().trim()).unwrap().as_str().parse().unwrap())
				.collect()
		})
	}
}
/// 出欠テーブル: 科目行
#[derive(Serialize, Deserialize)] #[serde(rename_all = "camelCase")]
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
#[derive(Serialize, Deserialize)] #[serde(rename_all = "camelCase")]
pub struct PeriodAttendanceRates
{
	#[doc = "初年度"] pub first_year: u32,
	#[doc = "開始時期"] pub starting_period: Period,
	#[doc = "出席率"] pub rates: Vec<f32>
}
/// 開講時期
#[derive(Serialize, Deserialize)]
pub enum Period
{
	#[doc = "通年"] WholeYear,
	#[doc = "前期"] FirstStage, #[doc = "後期"] LateStage,
	#[doc = "1Q"] FirstQuarter, #[doc = "2Q"] SecondQuarter, #[doc = "3Q"] ThirdQuarter, #[doc = "4Q"] FourthQuarter
}
/// 曜日
#[derive(Serialize, Deserialize)]
pub enum Week
{
	Monday, Tuesday, Wednesday, Thursday, Friday, Saturday
}
/// 出席状態
#[derive(Serialize, Deserialize)]
pub enum DayAttendanceState
{
	#[doc = "データなし"] NoData,
	#[doc = "出席"] Presence, #[doc = "欠席"] Absence, #[doc = "公認欠席"] Authorized
}
impl FromStr for Period
{
	type Err = ();
	fn from_str(s: &str) -> Result<Self, ()>
	{
		match s
		{
			"通年" => Ok(Period::WholeYear), "前期" => Ok(Period::FirstStage), "後期" => Ok(Period::LateStage),
			"1 Q" => Ok(Period::FirstQuarter), "2 Q" => Ok(Period::SecondQuarter), "3 Q" => Ok(Period::ThirdQuarter),
			"4 Q" => Ok(Period::FourthQuarter), _ => Err(())
		}
	}
}
impl FromStr for Week
{
	type Err = ();
	fn from_str(s: &str) -> Result<Self, ()>
	{
		match s
		{
			"月曜日" => Ok(Week::Monday), "火曜日" => Ok(Week::Tuesday), "水曜日" => Ok(Week::Wednesday),
			"木曜日" => Ok(Week::Thursday), "金曜日" => Ok(Week::Friday), "土曜日" => Ok(Week::Saturday),
			_ => Err(())
		}
	}
}
