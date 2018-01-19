#![feature(proc_macro, conservative_impl_trait, generators)]

#[macro_use]
extern crate error_chain;

extern crate futures_await as futures;
extern crate organelle;
extern crate tokio_core;

use futures::prelude::*;
use organelle::*;
use tokio_core::reactor;

enum TestSignal {
    Something,
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
enum TestSynapse {
    Something,
}

struct GiveSomethingSoma;

impl GiveSomethingSoma {
    fn sheath() -> Result<Sheath<Self>> {
        Sheath::new(
            Self {},
            vec![],
            vec![Dendrite::RequireOne(TestSynapse::Something)],
        )
    }
}

impl Neuron for GiveSomethingSoma {
    type Signal = TestSignal;
    type Synapse = TestSynapse;
    type Error = Error;
    type Future = Box<
        Future<
            Item = (Self, Axon<Self::Signal, Self::Synapse>),
            Error = Self::Error,
        >,
    >;

    #[async(boxed)]
    fn update(
        self,
        axon: Axon<Self::Signal, Self::Synapse>,
        msg: Impulse<Self::Signal, Self::Synapse>,
    ) -> Result<(Self, Axon<Self::Signal, Self::Synapse>)> {
        match msg {
            Impulse::Start => {
                axon.send_req_output(
                    TestSynapse::Something,
                    TestSignal::Something,
                )?;

                Ok((self, axon))
            },
            _ => bail!("unexpected message"),
        }
    }
}

struct TakeSomethingSoma;

impl TakeSomethingSoma {
    fn sheath() -> Result<Sheath<Self>> {
        Sheath::new(
            Self {},
            vec![Dendrite::RequireOne(TestSynapse::Something)],
            vec![],
        )
    }
}

impl Neuron for TakeSomethingSoma {
    type Signal = TestSignal;
    type Synapse = TestSynapse;
    type Error = Error;
    type Future = Box<
        Future<
            Item = (Self, Axon<Self::Signal, Self::Synapse>),
            Error = Self::Error,
        >,
    >;

    #[async(boxed)]
    fn update(
        self,
        axon: Axon<Self::Signal, Self::Synapse>,
        msg: Impulse<Self::Signal, Self::Synapse>,
    ) -> Result<(Self, Axon<Self::Signal, Self::Synapse>)> {
        match msg {
            Impulse::Start => Ok((self, axon)),

            Impulse::Signal(_, TestSignal::Something) => {
                axon.effector()?.stop();

                Ok((self, axon))
            },

            _ => bail!("unexpected message"),
        }
    }
}

#[test]
fn test_invalid_input() {
    let mut core = reactor::Core::new().unwrap();

    let mut organelle =
        Organelle::new(core.handle(), GiveSomethingSoma::sheath().unwrap());

    let give1 = organelle.get_main_handle();
    let give2 = organelle.add_soma(GiveSomethingSoma::sheath().unwrap());

    organelle.connect(give1, give2, TestSynapse::Something);

    if let Err(e) = core.run(organelle.into_future()) {
        eprintln!("error {:#?}", e)
    } else {
        panic!("GiveSomethingSoma should not accept this input")
    }
}

#[test]
fn test_require_one() {
    println!("make sure require one works as intended");
    {
        let mut core = reactor::Core::new().unwrap();

        let mut organelle =
            Organelle::new(core.handle(), GiveSomethingSoma::sheath().unwrap());

        let give = organelle.get_main_handle();
        let take = organelle.add_soma(TakeSomethingSoma::sheath().unwrap());

        organelle.connect(give, take, TestSynapse::Something);

        core.run(organelle.into_future()).unwrap();
    }

    println!("make sure require one fails as intended");
    {
        let mut core = reactor::Core::new().unwrap();

        let handle = core.handle();

        if let Err(e) = core.run(
            Organelle::new(
                handle.clone(),
                TakeSomethingSoma::sheath().unwrap(),
            ).into_future(),
        ) {
            eprintln!("error {:#?}", e)
        } else {
            panic!("TakeSomethingSoma has no input, so it should fail")
        }

        if let Err(e) = core.run(
            Organelle::new(
                handle.clone(),
                GiveSomethingSoma::sheath().unwrap(),
            ).into_future(),
        ) {
            eprintln!("error {:#?}", e)
        } else {
            panic!("GiveSomethingSoma has no output, so it should fail")
        }
    }
}
