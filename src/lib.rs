#![warn(missing_docs)]
#![feature(core_intrinsics, proc_macro, conservative_impl_trait, generators)]

//! Organelle - reactive architecture for emergent AI systems

#[macro_use]
extern crate error_chain;

extern crate futures_await as futures;
extern crate tokio;
extern crate tokio_core;
extern crate uuid;

mod axon;
mod organelle;
mod soma;

pub use axon::{Axon, Dendrite};
pub use organelle::Organelle;
pub use soma::{Impulse, Role, Soma, Synapse};
/// organelle error
error_chain! {
    foreign_links {
        Io(std::io::Error) #[doc = "glue for io::Error"];
        Canceled(futures::Canceled) #[doc = "glue for futures::Canceled"];
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
