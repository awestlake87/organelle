#![warn(missing_docs)]

//! Cortical - general purpose lobe networks

#[macro_use]
extern crate error_chain;

extern crate uuid;


#[macro_use]
mod cortex;
#[macro_use]
mod lobe;

pub use cortex::{
    CortexConstraints,
    CortexNode,
    CortexLink,
    NodeHdl,
    Cortex,
    CortexBuilder,
    CortexDistributor
};
pub use lobe::{ Lobe };



/// convert from cortex data
pub trait FromCortexData<D> where Self: Sized {
    /// convert from cortex data
    fn from_cortex_data(data: D) -> Result<Self>;
}

/// convert into cortex data
pub trait IntoCortexData<D> {
    /// convert into cortex data
    fn into_cortex_data(self) -> Result<D>;
}

/// cortical error
error_chain! {
    errors {
        /// a lobe returned an error when called into
        LobeError {
            description("an error occurred while calling into a lobe"),
            display("an error occurred while calling into a lobe")
        }
    }
}
