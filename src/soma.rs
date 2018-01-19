use std;
use std::fmt::Debug;
use std::hash::Hash;
use std::intrinsics;

use futures::prelude::*;

use super::{Error, Impulse};

/// defines the collection of traits necessary to act as a soma message
pub trait Signal: 'static {}

impl<T> Signal for T
where
    T: 'static,
{
}

/// defines the collection of traits necessary to act as a soma role
pub trait Synapse: Debug + Copy + Clone + Hash + Eq + PartialEq + 'static {}

impl<T> Synapse for T
where
    T: Debug + Copy + Clone + Hash + Eq + PartialEq + 'static,
{
}

/// defines an interface for a soma of any type
///
/// generic across the user-defined message to be passed between somas and the
/// user-defined roles for connections
pub trait Soma: Sized {
    /// user-defined message to be passed between somas
    type Signal: Signal;
    /// user-defined roles for connections
    type Synapse: Synapse;
    /// error when a soma fails to update
    type Error: std::error::Error + Send + From<Error> + 'static;
    /// future representing the update task
    type Future: Future<Item = Self, Error = Self::Error>;

    /// the name of the soma
    fn type_name() -> &'static str {
        unsafe { intrinsics::type_name::<Self>() }
    }

    /// apply any changes to the soma's state as a result of _msg
    fn update(self, _msg: Impulse<Self::Signal, Self::Synapse>)
        -> Self::Future;
}
