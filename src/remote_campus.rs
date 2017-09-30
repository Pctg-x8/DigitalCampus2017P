//! DigitalCampus Remote Controllers

use {headless_chrome, GenericResult};
use headless_chrome::{SessionEventSubscriber, SessionEventSubscribable};
use std::sync::atomic::{Ordering, AtomicUsize};
use std::net::TcpStream;
use serde_json; use serde_json::Value as JValue;
use regex::{Regex, Captures};
use std::marker::PhantomData;

pub struct RemoteCampus { session: headless_chrome::Session<TcpStream, TcpStream>, request_id: AtomicUsize }
impl RemoteCampus
{
	pub fn connect(addr: &str) -> GenericResult<Self>
	{
		let mut object = headless_chrome::Session::connect(addr).map(|session| RemoteCampus { session, request_id: AtomicUsize::new(1) })?;
		let objptr = &object as &SessionEventSubscriber<_> as *const _ as *mut _;
		unsafe { object.session.subscribe_session_event_raw(objptr) };
		object.session.page().enable(0).unwrap(); object.session.wait_result(0).unwrap();
		object.session.dom().enable(0).unwrap(); object.session.wait_result(0).unwrap();
		Ok(object)
	}
	fn new_request_id(&self) -> usize { self.request_id.fetch_add(1, Ordering::SeqCst) }

	pub fn query_page_location(&mut self) -> GenericResult<String>
	{
		let id = self.new_request_id();
		self.session.runtime().evaluate_sync(id, "location.href").map(|m| match m
		{
			JValue::Object(mut o) => match o.remove("result")
			{
				Some(JValue::Object(mut vo)) => match vo.remove("value")
				{
					Some(JValue::String(s)) => s, _ => api_corruption!(value_type)
				},
				_ => api_corruption!(value_type)
			},
			_ => api_corruption!(value_type)
		})
	}
	pub fn is_in_login_page(&mut self) -> GenericResult<bool>
	{
		self.query_page_location().map(|l| l.contains("/campuslogin"))
	}
	pub fn is_in_home(&mut self) -> GenericResult<bool>
	{
		self.query_page_location().map(|l| l.contains("/campusHomepage"))
	}

	pub fn click_element(&mut self, selector: &str) -> GenericResult<&mut Self>
	{
		let id = self.new_request_id();
		self.session.runtime().evaluate_sync(id, &format!(r#"document.querySelector({:?}).click()"#, selector)).map(move |_| self)
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
		println!("FrameNavigated: {}", event.url);
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
	pub fn jump_into_intersys(mut self) -> GenericResult<CampusPlanEntryFrames>
	{
		self.remote.jump_to_anchor_href(Self::INTERSYS_LINK_PATH)?;
		self.remote.sync()?;
		Ok(unsafe { self.remote.assume_cp_frame() })
	}
}

/// CampusPlan フレームページ
pub struct CampusPlanFrames<MainFrameCtrlTy: PageControl> { remote: RemoteCampus, ph: PhantomData<MainFrameCtrlTy> }
impl RemoteCampus
{
	pub unsafe fn assume_cp_frame<MainFrameCtrlTy: PageControl>(self) -> CampusPlanFrames<MainFrameCtrlTy>
	{
		CampusPlanFrames { remote: self, ph: PhantomData }
	}
}
pub type CampusPlanEntryFrames  = CampusPlanFrames<CampusPlanEntryPage>;
pub type CampusPlanCourseFrames = CampusPlanFrames<CampusPlanCoursePage>;
impl<MainFrameCtrlTy: PageControl> CampusPlanFrames<MainFrameCtrlTy>
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
}
pub struct CampusPlanEntryPage { remote: RemoteCampus }
impl CampusPlanEntryPage
{
	/// 履修関係セクションへ
	pub fn jump_into_course_category(mut self) -> GenericResult<CampusPlanCourseFrames>
	{
		self.remote.click_element("a#dgSystem__ctl2_lbtnSystemName")?; self.remote.sync()?;
		Ok(unsafe { self.remote.assume_cp_frame() })
	}
}
pub struct CampusPlanCoursePage { remote: RemoteCampus }
impl CampusPlanCoursePage
{
	/// 履修チェック結果の確認ページへ
	/// * 履修登録期間中はこれだと動かないかもしれない
	pub fn jump_into_course_details(mut self) -> GenericResult<CourseDetailsPage>
	{
		self.remote.click_element("#dgSystem__ctl2_lbtnPage")?.sync()?;
		Ok(unsafe { self.remote.assume_course_details() })
	}
}
pub trait PageControl: Sized { unsafe fn with_remote(r: RemoteCampus) -> Self; }
impl PageControl for CampusPlanEntryPage  { unsafe fn with_remote(r: RemoteCampus) -> Self { CampusPlanEntryPage  { remote: r } } }
impl PageControl for CampusPlanCoursePage { unsafe fn with_remote(r: RemoteCampus) -> Self { CampusPlanCoursePage { remote: r } } }

/// 履修確認ページ
pub struct CourseDetailsPage { remote: RemoteCampus }
impl RemoteCampus { pub unsafe fn assume_course_details(self) -> CourseDetailsPage { CourseDetailsPage { remote: self } } }
/// 学生プロファイル/履修データの解析周り
impl CourseDetailsPage
{
	/// 学生プロファイルテーブルの解析　　
	/// セルで罫線を表現するというわけのわからない仕組みのため偶数行だけ取るようにしてる　　
	/// 奇数列は項目の名前("学籍番号"とか)
	pub fn parse_profile(&mut self) -> GenericResult<StudentProfile>
	{
		let id = self.remote.new_request_id();
		let profile_rows_data = self.remote.session.runtime().evaluate_value_sync(id,
			r#"Array.prototype.map.call(document.querySelectorAll('#TableProfile tr:nth-child(2n) td:nth-child(2n)'), function(x){ return x.textContent; })"#)?;
		let regex_replace_encoded = Regex::new(r"\\u\{([0-9a-fA-F]{4})\}").unwrap();
		let mut profile_rows: Vec<_> = match profile_rows_data
		{
			JValue::Object(mut pro) => match pro.remove("result")
			{
				Some(JValue::Object(mut ro)) => match ro.remove("value")
				{
					Some(JValue::Array(va)) => va.into_iter().map(|v| match v
					{
						JValue::String(s) => regex_replace_encoded.replace_all(s.trim(), |cap: &Captures|
						{
							String::from_utf16(&[u16::from_str_radix(&cap[1], 16).unwrap()]).unwrap()
						}).into_owned(),
						_ => api_corruption!(value_type)
					}).collect(),
					_ => api_corruption!(value_type)
				},
				_ => api_corruption!(value_type)
			},
			_ => api_corruption!(value_type)
		};

		Ok(StudentProfile
		{
			id: profile_rows.remove(0), name: profile_rows.remove(0),
			course: profile_rows.remove(0), grade: profile_rows.remove(0),
			semester: profile_rows.remove(0), address: profile_rows
		})
	}
	/// 履修テーブル(前半クォーター分だけ)の取得(クラス名の段階でわかるけどこれで3Q4Qどっちも取れる)
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
		let id = self.remote.new_request_id();
		let course_table = match self.remote.session.runtime().evaluate_value_sync(id, r#"
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
		"#)?
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
							_ => api_corruption!(value_type)
						}).collect(),
						_ => api_corruption!(value_type)
					}).collect::<Vec<Vec<_>>>(),
					_ => api_corruption!(value_type)
				},
				_ => api_corruption!(value_type)
			},
			_ => api_corruption!(value_type)
		};

		Ok(CourseTable
		{
			first_quarter: course_table[0].chunks(6).map(ToOwned::to_owned).collect(),
			last_quarter: course_table[1].chunks(6).map(ToOwned::to_owned).collect()
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
