#[macro_use]
extern crate error_chain;

extern crate organelle;

use organelle::*;

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

    fn update(
        self,
        axon: &Axon<Self::Signal, Self::Synapse>,
        msg: Impulse<Self::Signal, Self::Synapse>,
    ) -> Result<Self> {
        match msg {
            Impulse::Start => {
                axon.send_req_output(
                    TestSynapse::Something,
                    TestSignal::Something,
                )?;

                Ok(self)
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

    fn update(
        self,
        axon: &Axon<Self::Signal, Self::Synapse>,
        msg: Impulse<Self::Signal, Self::Synapse>,
    ) -> Result<Self> {
        match msg {
            Impulse::Start => Ok(self),

            Impulse::Signal(_, TestSignal::Something) => {
                axon.effector()?.stop();

                Ok(self)
            },

            _ => bail!("unexpected message"),
        }
    }
}

#[test]
fn test_invalid_input() {
    let mut organelle = Organelle::new(GiveSomethingSoma::sheath().unwrap());

    let give1 = organelle.get_main_handle();
    let give2 = organelle.add_soma(GiveSomethingSoma::sheath().unwrap());

    organelle.connect(give1, give2, TestSynapse::Something);

    if let Err(e) = organelle.run() {
        eprintln!("error {:#?}", e)
    } else {
        panic!("GiveSomethingSoma should not accept this input")
    }
}

#[test]
fn test_require_one() {
    // make sure require one works as intended
    {
        let mut organelle =
            Organelle::new(GiveSomethingSoma::sheath().unwrap());

        let give = organelle.get_main_handle();
        let take = organelle.add_soma(TakeSomethingSoma::sheath().unwrap());

        organelle.connect(give, take, TestSynapse::Something);

        organelle.run().unwrap();
    }

    // make sure require one fails as intended
    {
        if let Err(e) = TakeSomethingSoma::sheath().unwrap().run() {
            eprintln!("error {:#?}", e)
        } else {
            panic!("TakeSomethingSoma has no input, so it should fail")
        }

        if let Err(e) = GiveSomethingSoma::sheath().unwrap().run() {
            eprintln!("error {:#?}", e)
        } else {
            panic!("GiveSomethingSoma has no output, so it should fail")
        }
    }
}
