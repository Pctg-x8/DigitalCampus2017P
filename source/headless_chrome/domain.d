module headless_chrome.domain;

import headless_chrome : Session;
import vibe.data.serialization : byName, optional;
import vibe.data.json : Json, serializeToJson, serializeToJsonString;

import std.meta : AliasSeq;
import std.traits : getUDAs, Fields, ReturnType;
import std.typecons : Nullable;

struct RPCMethod(ReturnType = void) { alias ReturnTy = ReturnType; string methodName; }
struct RPCEventMethod { string methodName; }
enum getRPCMethodName(T) = getUDAs!(T, RPCMethod)[0].methodName;
alias getRPCMethodReturnType(T) = getUDAs!(T, RPCMethod)[0].ReturnTy;
enum getRPCEventMethodName(T) = getUDAs!(T, RPCEventMethod)[0].methodName;
auto makeMethodCall(T)(ulong id, T params = T())
{
	static if(Fields!T.length <= 0)
		return ["id": Json(id), "method": Json(getRPCMethodName!T)].serializeToJsonString;
	else return [
		"params": serializeToJson(params), "id": Json(id), "method": Json(getRPCMethodName!T)
	].serializeToJsonString;
}

/// This domain exposes DOM read/write operations
struct DOM
{
	// methods //
	/// Enables DOM agent for the given page
	@RPCMethod!void("DOM.enable") struct Enable {}
	/// Disables DOM agent for the given page
	@RPCMethod!void("DOM.disable") struct Disable {}

	/// Returns the root DOM node to the caller
	@RPCMethod!Node("DOM.getDocument")
	struct GetDocument {}
	/// Executes `querySelector` on a given node
	@RPCMethod!NodeId("DOM.querySelector")
	struct QuerySelector
	{
		/// Id of the node to query upon
		long nodeId;
		/// Selector string
		string selector;
	}
	/// Executes `querySelectorAll` on a given node
	@RPCMethod!(NodeId[])("DOM.querySelectorAll")
	struct QuerySelectorAll
	{
		/// Id of the node to query upon
		long nodeId;
		/// Selector string
		string selector;
	}
	version(Experimental) @RPCMethod("DOM.focus")
	struct Focus { long nodeId; }
	/// Returns attributes for the specified node
	@RPCMethod!(string[])("DOM.getAttributes")
	struct GetAttributes
	{
		/// Id of the node to retrieve attributes for
		long nodeId;
	}

	// typedefs //
	/// Unique DOM node identifier
	alias NodeId = long;
	/// Pseudo element type
	@byName enum PseudoType
	{
		first_line, first_letter, before, after, backdrop, selection, first_line_inherited,
		scrollbar, scrollbar_thumb, scrollbar_button, scrollbar_track, scrollbar_track_piece,
		scrollbar_corner, resizer, input_list_button
	}
	/// Shadow root type
	@byName enum ShadowRootType { user_agent, open, closed }
	/// DOM interaction is implemented in terms of mirror objects that represent the
	/// actual DOM nodes. DOMNode is a base node mirror type
	struct Node
	{
		/// Node identifier that is passed into the rest of the DOM messages as the `nodeId`
		NodeId nodeId;
		/// `Node`'s nodeType
		long nodeType;
		/// `Node`'s nodeName
		string nodeName;
		/// `Node`'s localName
		string localName;
		/// `Node`'s nodeValue
		string nodeValue;
		/// Child count for `Container` nodes
		@optional long childNodeCount;
		/// Child nodes of this node when requested with children
		@optional Node[] children;
		/// Attributes of the `Element` node in the form of flat array `[name1, value1, name2, value2]`
		@optional string[] attributes;
		/// Document URL that `Document` or `FrameOwner` node points to
		@optional string documentURL;
		/// Base URL that `Document` or `FrameOwner` node uses for URL completion
		version(Experimental) @optional string baseURL;
		/// `DocumentType`'s publicId
		@optional string publicId;
		/// `DocumentType`'s systemId
		@optional string systemId;
		/// `DocumentType`'s internalSubset
		@optional string internalSubset;
		/// `Document`'s XML version in case of XML documents
		@optional string xmlVersion;
		/// `Attr`'s name
		@optional string name;
		/// `Attr`'s value
		@optional string value;
		/// Pseudo element type for this node
		@optional PseudoType pseudoType;
		/// Shadow root type
		@optional ShadowRootType shadowRootType;
		/// Frame ID for frame owner elements
		version(Experimental) @optional Page.FrameId frameId;
		/// Content document for frame owner elements
		// @optional Node contentDocument;
		/// Shadow root list for given element host
		// version(Experimental) @optional Node[] shadowRoots;
		/// Content document fragment for template elements
		version(Experimental) @optional Node templateContent;
		/// Pseudo elements associated with this node
		// version(Experimental) @optional Node[] pseudoElements;
		/// Import document for the HTMLImport links
		// @optional Node importedDocument;
		/// Distributed nodes for given intertion point
		version(Experimental) @optional BackendNode[] distributedNodes;
	}
}

/// Input Domain
struct Input
{
	/// Dispatches a key event to the page
	@RPCMethod!void("Input.dispatchKeyEvent")
	struct DispatchKeyEvent { KeyEvent type; string text; }

	/// Type of the key event
	@byName enum KeyEvent { keyDown, keyUp, rawKeyDown, char_ }
}

/// [stub]Network domain allows tracking network activities of the page
struct Network
{
	/// Unique loader identifier
	alias LoaderId = string;
}

/// Actions and events related to the inspected page belong to the page domain
struct Page
{
	// methods //
	/// Enables page domain notifications
	@RPCMethod!void("Page.enable") struct Enable {}
	/// Disables page domain notifications
	@RPCMethod!void("Page.disable") struct Disable {}

	/// Navigates current page to the given URL
	@RPCMethod!FrameId("Page.navigate")
	struct Navigate { string url; }
	version(Experimental) @RPCMethod("getResourceTree")
	struct GetResourceTree {}
	version(Experimental) @RPCMethod("createIsolatedWorld")
	struct CreateIsolatedWorld { string frameId; }

	// events //
	@RPCEventMethod("Page.loadEventFired")
	struct LoadEventFired { float timestamp; }
	/// Fired once navigation of the frame has completed.
	/// Frame is now associated with the new loader
	@RPCEventMethod("Page.frameNavigated")
	struct FrameNavigated
	{
		/// Frame object
		Frame frame;
	}

	// typedefs //
	/// Resource type as it was perceived by the rendering engine
	@byName enum ResourceType
	{
		Document, Stylesheet, Image, Media, Font, Script, TextTrack, XHR,
		Fetch, EventSource, WebSocket, Manifest, Other
	}
	/// Unique frame identifier
	alias FrameId = string;
	/// Information about the Frame on the page
	struct Frame
	{
		/// Frame unique identifier
		string id;
		/// Parent frame identifier
		string parentId;
		/// Identifier of the loader associated with this frame
		Network.LoaderId loaderId;
		/// Frame's name as specified in the tag
		@optional string name;
		/// Frame document's URL
		string url;
		/// Frame document's security origin
		string securityOrigin;
		/// Frame document's mimeType as determined by the browser
		string mimeType;
	}
}

/// Javascript Runtime Domain
struct Runtime
{
	// methods //
	/// Enables reporting of execution contexts creation by means of `executionContextCreated` event
	@RPCMethod!void("Runtime.enable") struct Enable {}
	/// Disables reporting of execution contexts creation
	@RPCMethod!void("Runtime.disable") struct Disable {}

	/// Evaluates expression on global object
	@RPCMethod!EvaluateResult("Runtime.evaluate")
	struct Evaluate
	{
		/// Expression to evaluate
		string expression;
		/// Whether the result is expected to be a JSON object that should be sent by value
		bool returnByValue;
	}
	/// Evaluates expression on global object
	@RPCMethod!EvaluateResult("Runtime.evaluate")
	struct EvaluateIn
	{
		/// Expression to evaluate
		string expression;
		/// Specified in which execution context to perform evaluation
		ulong contextId;
		/// Whether the result is expected to be a JSON object that should be sent by value
		bool returnByValue;
	}
	/// Returns properties ofa given object
	@RPCMethod!GetPropertiesResult("Runtime.getProperties")
	struct GetProperties
	{
		/// Identifier of the object to return properties for
		RemoteObjectId objectId;
	}

	// events //
	/// Issued when new execution context is created
	@RPCEventMethod("Runtime.executionContextCreated")
	struct ExecutionContextCreated
	{
		/// A newly created execution context
		ExecutionContextDescription context;
	}
	/// Issued when execution context is destroyed
	@RPCEventMethod("Runtime.executionContextDestroyed")
	struct ExecutionContextDestroyed
	{
		/// Id of the destroyed context
		ExecutionContextId executionContextId;
	}
	/// Issued when all executionContexts were cleared in browser
	@RPCEventMethod("Runtime.executionContextsCleared")
	struct ExecutionContextsCleared;

	// typedefs //
	/// Unique script identifier
	alias ScriptId = string;
	/// Unique object identifier
	alias RemoteObjectId = string;
	/// Mirror object referencing original JavaScript object
	struct RemoteObject
	{
		/// Object type
		@byName ObjectType type;
		/// Object subtype hint
		@optional @byName ObjectSubtype subtype;
		/// Object class(constructor) name
		@optional string className;
		/// Remote object value in case of primitive values or JSON values(if it was requested)
		@optional Json value;
		/// Unique object identifier(for non-primitive values)
		@optional RemoteObjectId objectId;
	}
	/// Object property descriptor
	struct PropertyDescriptor
	{
		/// Property name or symbol description
		string name;
		/// The value associated with the property
		@optional RemoteObject value;
		/// True if the value associated with the property may be changed
		@optional bool writable;
		/// A function which serves as a getter for the property, or `undefined` if there is no getter
		@optional RemoteObject get;
		/// A function which serves as a setter for the property, or `undefined` if there is no setter
		@optional RemoteObject set;
		/// True if the type of this property descriptor may be changed and if the property may be
		/// deleted from the corresponding object
		bool configurable;
		/// True if the proeprty shows up during enumeration of the properties on the corresponding object
		bool enumerable;
		/// True if the result was thrown during the evaluation
		@optional bool wasThrown;
		/// True if the Property is owned for the object
		@optional bool isOwn;
		/// Property symbol object, if the property is of the `symbol` type
		@optional RemoteObject symbol;
	}
	/// Object internal property descriptor
	struct InternalPropertyDescriptor
	{
		/// Conventional property name
		string name;
		/// The value associated with the property
		@optional RemoteObject value;
	}
	/// Id of an execution context
	alias ExecutionContextId = ulong;
	/// Description of an isolated world
	struct ExecutionContextDescription
	{
		/// Unique id of the execution context
		ExecutionContextId id;
		/// Execution context origin
		string origin;
		/// Human readable name describing given context
		string name;
		/// Embedder-specific auxiliary data
		Json auxData;
	}
	/// Detailed information about exception(or error) that was thrown during script compilation or execution
	struct ExceptionDetails
	{
		/// Exception id
		ulong exceptionId;
		/// Exception text, which should be used together with exception object when available
		string text;
		/// Line number of the exception location(0-based)
		ulong lineNumber;
		/// Column number of the exception location(0-based)
		ulong columnNumber;
		/// Script ID of the exception location
		@optional ScriptId scriptId;
		/// URL of the exception location, to be used when the script was not reported
		@optional string url;
	}

	/// Result set of the `evaluate` method
	struct EvaluateResult
	{
		/// Evaluation result
		RemoteObject result;
		/// Execption details
		@optional ExceptionDetails exceptionDetails;
	}
	/// Result set of the `getProperties` method
	struct GetPropertiesResult
	{
		/// Object properties
		PropertyDescriptor[] result;
		/// Internal object properties(only of the element itself)
		@optional InternalPropertyDescriptor[] internalProperties;
		/// Exception details
		@optional ExceptionDetails exceptionDetails;
	}
	enum ObjectType { object, function_, undefined, string, number, boolean, symbol }
	enum ObjectSubtype
	{
		array, null_, node, regexp, date, map, set, iterator, generator, error, proxy, promise, typedarray
	}
}
