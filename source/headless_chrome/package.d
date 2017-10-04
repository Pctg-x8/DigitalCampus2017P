/** An interface to the Headless Chrome */
module headless_chrome;

public import domain = headless_chrome.domain;

import std.conv : to;
import std.process : spawnProcess, Pid, kill;
import std.traits : Fields;
import vibe.data.json : deserializeJson, serializeToJsonString, Json;
import vibe.data.serialization : name;
import vibe.inet.url : URL;
import vibe.http.client : requestHTTP;
import vibe.http.websockets : connectWebSocket, WebSocket;

/// Headless Chrome
final class Process
{
	private Pid pid;
	private ushort port;

	/// Version information of the browser
	struct VersionInfo
	{
		/// Protocol version
		@name("Protocol-Version") public string protocolVersion;
		/// Rendering engine version
		@name("WebKit-Version") public string webkitVersion;
		/// Browser version
		@name("Browser") public string browser;
		/// UserAgent used in browser
		@name("User-Agent") public string userAgent;
		/// JavaScript engine version
		@name("V8-Version") public string v8Version;
	}

	version(Windows) private static immutable DEFAULT_BIN = `C:\Program Files (x86)\Google\Chrome\Application\chrome.exe`;
	else private static immutable DEFAULT_BIN = "google-chrome-stable";

	/// Launch chrome, with accessing `home` and debug through port `port`
	this(in string home, ushort port = 9222)
	{
		import std.stdio : writeln;
		import std.process : environment;

		if("CHROME_BIN" !in environment) writeln("Warning: $CHROME_BIN is not set, defaulting to ", DEFAULT_BIN);
		const chrome_bin = environment.get("CHROME_BIN", DEFAULT_BIN);
		const cline = [chrome_bin, "--headless", "--disable-gpu", "--remote-debugging-port=" ~ port.to!string, home];
		writeln("Launching Chrome... ", cline);
		this.port = port;
		this.pid = spawnProcess(cline);
	}
	~this() { this.pid.kill(); }

	private @property host() const { return "http://127.0.0.1:" ~ this.port.to!string; }
	/// Request the version info of the chrome
	@property versions() const
	{
		return requestHTTP(this.host ~ "/json/version").readJson().deserializeJson!VersionInfo();
	}
	/// Request a list of sessions
	@property sessions() const
	{
		return requestHTTP(this.host ~ "/json").readJson();
	}
}

/// A session in the Headless Chrome
final class Session
{
	private WebSocket sock;

	/// Connect via WebSocket
	this(string url)
	{
		this.sock = connectWebSocket(URL(url));
	}

	/// Send a method
	void send(T)(ulong id, T params = T())
	{
		debug(VerboseStream)
		{
			import std.stdio : writeln;
			const v = domain.makeMethodCall(id, params);
			writeln("[send]", v);
			this.sock.send(v);
		}
		else this.sock.send(domain.makeMethodCall(id, params));
	}
	/// Send a method and wait a returned value
	auto sendSync(T)(ulong id, T params = T())
	{
		this.send(id, params);
		return this.waitForResult!(domain.getRPCMethodReturnType!T)(id);
	}
	/// Wait an event
	T waitForEvent(T)()
	{
		while(true)
		{
			debug(VerboseStream)
			{
				import std.stdio : writeln;
				const rt = this.sock.receiveText();
				writeln("[waitForEvent]", rt);
				const r = deserializeJson!(Json[string])(rt);
			}
			else
			{
				const r = this.sock.receiveText().deserializeJson!(Json[string])();
			}
			if(const mtd = "method" in r)
			{
				if(*mtd == domain.getRPCEventMethodName!T)
				{
					static if(Fields!T.length <= 0) return T();
					else return deserializeJson!T(r["params"]);
				}
			}
			else if(const e = "error" in r) throw new RPCError(*e);
		}
	}
	/// Wait a result of specified id
	T waitForResult(T)(ulong id)
	{
		while(true)
		{
			debug(VerboseStream)
			{
				import std.stdio : writeln;
				const rt = this.sock.receiveText();
				writeln("[waitForResult]", rt);
				const r = deserializeJson!(Json[string])(rt);
			}
			else
			{
				const r = this.sock.receiveText().deserializeJson!(Json[string])();
			}
			if(const res = "result" in r)
			{
				if(r["id"] == id)
				{
					static if(is(T == void)) return;
					else static if(Fields!T.length <= 0) return T();
					else return deserializeJson!T(*res);
				}
			}
			else if(const e = "error" in r) throw new RPCError(*e);
		}
	}
}

/// Error from the browser while processing method
final class RPCError : Exception
{
	/// Error code
	long code;
	/// Describing an error
	string message;
	/// Calling ID
	ulong id;

	/// Construct from an object containing informations of an error
	this(Json obj)
	{
		this.code = obj["code"].to!long;
		this.message = obj["message"].to!string;
		this.id = obj["id"].to!ulong;
		super("RPC Error(" ~ this.code.to!string ~ "): " ~ this.message ~ " in processing id " ~ this.id.to!string);
	}
}
