//! DigitalCampus Scraper Backend

extern crate hyper; extern crate futures;
extern crate websocket;
extern crate serde; extern crate serde_json;
#[macro_use] extern crate serde_derive;
extern crate chrono;

#[cfg(feature = "verbose")] extern crate colored;

// common defs //
type GenericResult<T> = Result<T, Box<std::error::Error>>;
macro_rules! api_corruption
{
	(value_type) => (panic!("Unexpected value type returned. the API may be corrupted"));
	(invalid_format) => (panic!("Invalid JSON format. the API may be corrupted"))
}

pub mod headless_chrome;
#[macro_use] mod jsquery;
mod remote_campus;

pub use remote_campus::*;
