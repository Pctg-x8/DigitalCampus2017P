//! JavaScript Fragment Combinator

use std::fmt::{Display, Result as FmtResult, Formatter};
use std::marker::PhantomData;

pub mod types
{
	use std::marker::PhantomData;
	use std::fmt::{Display, Result as FmtResult, Formatter};
	
	pub enum Element {}
	pub enum NodeList {}
	pub enum String {}
	pub enum Object {}
	pub struct Array<T>(PhantomData<T>);
	pub struct Closure<T>(PhantomData<T>);

	pub trait QueryableElements {}
	impl QueryableElements for Element {}
	pub trait Callable { type ReturnTy; }
	impl<T> Callable for Closure<T> { type ReturnTy = T; }
	pub trait Iterable
	{
		fn js_format<Src: Display, MapFn: Display>(source: &Src, mapfn: &MapFn, formatter: &mut Formatter) -> FmtResult;
	}
	impl Iterable for NodeList
	{
		fn js_format<Src: Display, MapFn: Display>(source: &Src, mapfn: &MapFn, formatter: &mut Formatter) -> FmtResult
		{
			write!(formatter, "Array.prototype.map.call({}, {})", source, mapfn)
		}
	}
	impl<T> Iterable for Array<T>
	{
		fn js_format<Src: Display, MapFn: Display>(source: &Src, mapfn: &MapFn, formatter: &mut Formatter) -> FmtResult
		{
			write!(formatter, "({}).map({})", source, mapfn)
		}
	}
}
// basic fragments //
/// The `#document` element representation in JavaScript
pub struct Document;
/// Custom typed expression
pub struct CustomExpression<T>(pub String, pub PhantomData<T>);
/// Closure(Arrow) expression
pub struct Closure<'s, InnerTy: QueryCombinator>(&'s str, InnerTy);

// specialized ops //
/// method calling syntax of `JSON.stringify(Any)`
pub struct ObjectStringify<InnerTy: QueryCombinator>(InnerTy);
/// Element.querySelector
pub struct QuerySelector<ParentTy: QueryCombinator>(ParentTy, String) where ParentTy::ValueTy: types::QueryableElements;
/// Element.querySelectorAll
pub struct QuerySelectorAll<ParentTy: QueryCombinator>(ParentTy, String)
	where ParentTy::ValueTy: types::QueryableElements;
/// Single value mapping operation(`({closure})({expr})`)
pub struct ValueMapping<InnerTy: QueryCombinator, ClosureTy: QueryCombinator>(InnerTy, ClosureTy)
	where ClosureTy::ValueTy: types::Callable;
/// Multiple value mapping operation(`{expr}.map({closure})` or `Array.prototype.map.call({expr}, {closure})`)
pub struct Mapping<SourceTy: QueryCombinator, ClosureTy: QueryCombinator>(SourceTy, ClosureTy)
	where SourceTy::ValueTy: types::Iterable, ClosureTy::ValueTy: types::Callable;
pub struct ObjectConstructor<'v, 's: 'v>(pub &'v [(&'s str, &'s str)]);

/// Helper macro constructing ObjectConstructor
macro_rules! jsqGenObject
{
	{ $($k: ident : $v: expr),* } => { $crate::jsquery::ObjectConstructor(&[$((stringify!($k), $v)),*]) }
}
/// Helper macro constructing CustomExpression
macro_rules! jsqCustomExpr
{
	([$t: ty] $e: expr) => { $crate::jsquery::CustomExpression::<$t>($e.into(), PhantomData) }
}

/// Lazy-combined: JavaScript Fragment Combinator
pub trait QueryCombinator: Sized + ::std::fmt::Display
{
	/// Expecting Value Type of this expression
	type ValueTy;

	// querying ops //
	fn query_selector(self, selector: String) -> QuerySelector<Self> where Self::ValueTy: types::QueryableElements
	{
		QuerySelector(self, selector)
	}
	fn query_selector_all(self, selector: String) -> QuerySelectorAll<Self> where Self::ValueTy: types::QueryableElements
	{
		QuerySelectorAll(self, selector)
	}

	fn into_closure<'s>(self, arg: &'s str) -> Closure<'s, Self> { Closure(arg, self) }
	fn stringify(self) -> ObjectStringify<Self> { ObjectStringify(self) }
	fn map_value<ClosureTy: QueryCombinator>(self, mapfn: ClosureTy) -> ValueMapping<Self, ClosureTy>
		where ClosureTy::ValueTy: types::Callable
	{
		ValueMapping(self, mapfn)
	}
	fn map_value_auto<'s, ExpressionTy: QueryCombinator>(self, bound: &'s str, expr: ExpressionTy)
		-> ValueMapping<Self, Closure<'s, ExpressionTy>>
	{
		self.map_value(expr.into_closure(bound))
	}
	fn map<ClosureTy: QueryCombinator>(self, mapfn: ClosureTy) -> Mapping<Self, ClosureTy>
		where Self::ValueTy: types::Iterable, ClosureTy::ValueTy: types::Callable
	{
		Mapping(self, mapfn)
	}
	fn map_auto<'s, ExpressionTy: QueryCombinator>(self, bound: &'s str, expr: ExpressionTy)
		-> Mapping<Self, Closure<'s, ExpressionTy>> where Self::ValueTy: types::Iterable
	{
		self.map(expr.into_closure(bound))
	}

	fn with_header(&self, header: &str) -> String { format!("{}\n{}", header, self) }
}
impl QueryCombinator for Document { type ValueTy = types::Element; }
impl<ParentTy: QueryCombinator> QueryCombinator for QuerySelector<ParentTy>
	where ParentTy::ValueTy: types::QueryableElements
{
	type ValueTy = types::Element;
}
impl<ParentTy: QueryCombinator> QueryCombinator for QuerySelectorAll<ParentTy>
	where ParentTy::ValueTy: types::QueryableElements
{
	type ValueTy = types::NodeList;
}
impl<InnerTy: QueryCombinator> QueryCombinator for ObjectStringify<InnerTy>
{
	type ValueTy = types::String;
}
impl<T> QueryCombinator for CustomExpression<T> { type ValueTy = T; }
impl<'s, InnerTy: QueryCombinator> QueryCombinator for Closure<'s, InnerTy>
{
	type ValueTy = types::Closure<InnerTy::ValueTy>;
}
impl<InnerTy: QueryCombinator, ClosureTy: QueryCombinator> QueryCombinator for ValueMapping<InnerTy, ClosureTy>
	where ClosureTy::ValueTy: types::Callable { type ValueTy = <ClosureTy::ValueTy as types::Callable>::ReturnTy; }
impl<SourceTy: QueryCombinator, ClosureTy: QueryCombinator> QueryCombinator for Mapping<SourceTy, ClosureTy>
	where SourceTy::ValueTy: types::Iterable, ClosureTy::ValueTy: types::Callable
{
	type ValueTy = types::Array<<ClosureTy::ValueTy as types::Callable>::ReturnTy>;
}
impl<'v, 's> QueryCombinator for ObjectConstructor<'v, 's> { type ValueTy = types::Object; }

// generate script //
impl<T> Display for CustomExpression<T> { fn fmt(&self, fmt: &mut Formatter) -> FmtResult { self.0.fmt(fmt) } }
impl Display for Document { fn fmt(&self, fmt: &mut Formatter) -> FmtResult { write!(fmt, "document") } }
impl<'s, InnerTy: QueryCombinator> Display for Closure<'s, InnerTy>
{
	fn fmt(&self, fmt: &mut Formatter) -> FmtResult { write!(fmt, "{} => {}", self.0, self.1) }
}
impl<ParentTy: QueryCombinator> Display for QuerySelector<ParentTy>
	where ParentTy::ValueTy: types::QueryableElements
{
	fn fmt(&self, fmt: &mut Formatter) -> FmtResult
	{
		write!(fmt, "({}).querySelector({:?})", self.0, self.1)
	}
}
impl<ParentTy: QueryCombinator> Display for QuerySelectorAll<ParentTy>
	where ParentTy::ValueTy: types::QueryableElements
{
	fn fmt(&self, fmt: &mut Formatter) -> FmtResult
	{
		write!(fmt, "({}).querySelectorAll({:?})", self.0, self.1)
	}
}
impl<InnerTy: QueryCombinator> Display for ObjectStringify<InnerTy>
{
	fn fmt(&self, fmt: &mut Formatter) -> FmtResult { write!(fmt, "JSON.stringify({})", self.0) }
}
impl<SourceTy: QueryCombinator, ClosureTy: QueryCombinator> Display for ValueMapping<SourceTy, ClosureTy>
	where ClosureTy::ValueTy: types::Callable
{
	fn fmt(&self, fmt: &mut Formatter) -> FmtResult { write!(fmt, "({})({})", self.1, self.0) }
}
impl<SourceTy: QueryCombinator, ClosureTy: QueryCombinator> Display for Mapping<SourceTy, ClosureTy>
	where SourceTy::ValueTy: types::Iterable, ClosureTy::ValueTy: types::Callable
{
	fn fmt(&self, fmt: &mut Formatter) -> FmtResult { <SourceTy::ValueTy as types::Iterable>::js_format(&self.0, &self.1, fmt) }
}
impl<'v, 's> Display for ObjectConstructor<'v, 's>
{
	fn fmt(&self, fmt: &mut Formatter) -> FmtResult
	{
		write!(fmt, "({{ {} }})", self.0.iter().map(|&(ref k, ref v)| format!("{}: {}", k, v)).collect::<Vec<String>>().join(","))
	}
}
