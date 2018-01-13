
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

struct GiveSomethingSoma {
    soma:           Axon<TestSignal, TestSynapse>,
}

impl GiveSomethingSoma {
    fn new() -> Result<Self> {
        Ok(
            Self {
                soma: Axon::new(
                    vec![ ],
                    vec![ Dendrite::RequireOne(TestSynapse::Something) ]
                )?
            }
        )
    }
}

impl Soma for GiveSomethingSoma {
    type Signal = TestSignal;
    type Synapse = TestSynapse;

    fn update(mut self, msg: Impulse<Self::Signal, Self::Synapse>)
        -> Result<Self>
    {
        if let Some(msg) = self.soma.update(msg)? {
            match msg {
                Impulse::Start => {
                    self.soma.send_req_output(
                        TestSynapse::Something, TestSignal::Something
                    )?;
                },
                _ => bail!("unexpected message"),
            }
        }

        Ok(self)
    }
}

struct TakeSomethingSoma {
    soma:           Axon<TestSignal, TestSynapse>,
}

impl TakeSomethingSoma {
    fn new() -> Result<Self> {
        Ok(
            Self {
                soma: Axon::new(
                    vec![ Dendrite::RequireOne(TestSynapse::Something) ],
                    vec![ ]
                )?
            }
        )
    }
}

impl Soma for TakeSomethingSoma {
    type Signal = TestSignal;
    type Synapse = TestSynapse;

    fn update(mut self, msg: Impulse<Self::Signal, Self::Synapse>)
        -> Result<Self>
    {
        if let Some(msg) = self.soma.update(msg)? {
            match msg {
                Impulse::Start => (),

                Impulse::Signal(_, TestSignal::Something) => {
                    self.soma.effector()?.stop();
                },

                _ => bail!("unexpected message"),
            }
        }

        Ok(self)
    }
}

#[test]
fn test_invalid_input() {
    let mut organelle = Organelle::new(GiveSomethingSoma::new().unwrap());

    let give1 = organelle.get_main_handle();
    let give2 = organelle.add_soma(GiveSomethingSoma::new().unwrap());

    organelle.connect(give1, give2, TestSynapse::Something);

    if let Err(e) = organelle.run() {
        eprintln!("error {:#?}", e)
    }
    else {
        panic!("GiveSomethingSoma should not accept this input")
    }
}

#[test]
fn test_require_one() {
    // make sure require one works as intended
    {
        let mut organelle = Organelle::new(GiveSomethingSoma::new().unwrap());

        let give = organelle.get_main_handle();
        let take = organelle.add_soma(TakeSomethingSoma::new().unwrap());

        organelle.connect(give, take, TestSynapse::Something);

        organelle.run().unwrap();
    }

    // make sure require one fails as intended
    {
        if let Err(e) = TakeSomethingSoma::new().unwrap().run() {
            eprintln!("error {:#?}", e)
        }
        else {
            panic!("TakeSomethingSoma has no input, so it should fail")
        }

        if let Err(e) = GiveSomethingSoma::new().unwrap().run() {
            eprintln!("error {:#?}", e)
        }
        else {
            panic!("GiveSomethingSoma has no output, so it should fail")
        }
    }
}
