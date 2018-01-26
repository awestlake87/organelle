#![warn(missing_docs)]
#![feature(core_intrinsics, proc_macro, conservative_impl_trait, generators)]

//! Organelle - reactive architecture for emergent AI systems

#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate serde_derive;

extern crate futures_await as futures;
extern crate serde;
extern crate serde_json;
extern crate tokio;
extern crate tokio_core;
extern crate uuid;

#[cfg(feature = "visualizer")]
extern crate hyper;
#[cfg(feature = "visualizer")]
extern crate hyper_staticfile;
#[cfg(feature = "visualizer")]
extern crate open;

mod axon;
mod organelle;
mod soma;

#[cfg(feature = "visualizer")]
pub mod visualizer;

pub mod probe;

pub use axon::{Axon, Constraint};
pub use organelle::Organelle;
pub use probe::ProbeData;
pub use soma::{Impulse, Soma, Synapse};

/// organelle error
error_chain! {
    foreign_links {
        Io(std::io::Error) #[doc = "glue for io::Error"];

        Canceled(futures::Canceled) #[doc = "glue for futures::Canceled"];
        SerdeJson(serde_json::Error) #[doc = "glue for serde_json::Error"];


        Hyper(hyper::Error)
            #[cfg(feature = "visualizer")]
            #[doc = "glue for hyper::Error"];

        AddrParse(std::net::AddrParseError)
            #[cfg(feature = "visualizer")]
            #[doc = "glue for net::AddrParseError"];
    }
    errors {
        /// a soma returned an error when called into
        SomaError {
            description("an error occurred while calling into a soma"),
            display("an error occurred while calling into a soma")
        }

        /// axon failed to validate a synapse
        InvalidSynapse(msg: String) {
            description("invalid synapse given to somas"),
            display("invalid synapse given to somas - {}", msg)
        }

        /// axon is missing a synapse
        MissingSynapse(msg: String) {
            description("missing synapse"),
            display("invalid synapse - {}", msg)
        }
    }
}

impl From<Error> for hyper::Error {
    fn from(e: Error) -> Self {
        hyper::Error::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("{:?}", e),
        ))
    }
}
