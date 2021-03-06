#[macro_use]
extern crate derive_builder;
#[macro_use] extern crate failure;

#[allow(unused_imports)]
#[macro_use]
extern crate clap;

#[allow(unused_imports)]
#[macro_use]
extern crate lazy_static;
#[macro_use] extern crate itertools;

// extern crate librsync;

extern crate rand;
// extern crate rustsync;

extern crate adler32;
extern crate blake2_rfc;
extern crate time;

#[macro_use]
extern crate rusqlite;


// extern crate futures;
// #[cfg(test)]
// extern crate rand;
// extern crate serde;
// #[macro_use]
// extern crate serde_derive;
// #[cfg(test)]
// extern crate tokio_core;
// extern crate tokio_io;

pub mod actions;
pub mod data_shape;
pub mod develope;
pub mod log_util;
pub mod rustsync;
pub mod db_accesses;
pub mod protocol;