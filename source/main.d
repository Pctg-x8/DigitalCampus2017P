import std.process;
import std.socket, std.stdio;
import std.conv;

import vibe.http.client, vibe.http.websockets;
import vibe.data.serialization;
import vibe.data.json;

import hc = headless_chrome;
import headless_chrome.domain : Page, Runtime;

void main()
{
	writeln("DigitalCampus 2017 Prototype");

	scope chrome = new hc.Process("https://dh.force.com/digitalCampus/campusHomepage");
	const cv = chrome.versions;
	writeln("Headless Chrome: ", cv.browser, " :: ", cv.protocolVersion);
	writeln("  webkit: ", cv.webkitVersion);
	writeln("  user-agent: ", cv.userAgent);

	const session_url = chrome.sessions[0]["webSocketDebuggerUrl"].get!string;
	writeln("Connecting to session ", session_url, "...");
	auto session = new hc.Session(session_url);
	session.sendSync!(Page.Enable)(0);
	session.sendSync!(Runtime.Enable)(0);
	session.waitForEvent!(Page.LoadEventFired)();
	const loc = session.sendSync(1, Runtime.Evaluate("location.href")).result.value.to!string;
	writeln("initial location: ", loc);
}
