
#[macro_use]
extern crate error_chain;

extern crate cortical;

use cortical::*;

enum TestMessage {
    Something,
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
enum TestRole {
    Something,
}

struct GiveSomethingLobe {
    soma:           Soma<TestMessage, TestRole>,
}

impl GiveSomethingLobe {
    fn new() -> Result<Self> {
        Ok(
            Self {
                soma: Soma::new(
                    vec![ ],
                    vec![ Constraint::RequireOne(TestRole::Something) ]
                )?
            }
        )
    }
}

impl Lobe for GiveSomethingLobe {
    type Message = TestMessage;
    type Role = TestRole;

    fn update(mut self, msg: Protocol<Self::Message, Self::Role>)
        -> Result<Self>
    {
        if let Some(msg) = self.soma.update(msg)? {
            match msg {
                Protocol::Start => {
                    self.soma.send_req_output(
                        TestRole::Something, TestMessage::Something
                    )?;
                },
                _ => bail!("unexpected message"),
            }
        }

        Ok(self)
    }
}

struct TakeSomethingLobe {
    soma:           Soma<TestMessage, TestRole>,
}

impl TakeSomethingLobe {
    fn new() -> Result<Self> {
        Ok(
            Self {
                soma: Soma::new(
                    vec![ Constraint::RequireOne(TestRole::Something) ],
                    vec![ ]
                )?
            }
        )
    }
}

impl Lobe for TakeSomethingLobe {
    type Message = TestMessage;
    type Role = TestRole;

    fn update(mut self, msg: Protocol<Self::Message, Self::Role>)
        -> Result<Self>
    {
        if let Some(msg) = self.soma.update(msg)? {
            match msg {
                Protocol::Start => (),
                
                Protocol::Message(_, TestMessage::Something) => {
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
    let mut cortex = Cortex::new(GiveSomethingLobe::new().unwrap());

    let give1 = cortex.get_main_handle();
    let give2 = cortex.add_lobe(GiveSomethingLobe::new().unwrap());

    cortex.connect(give1, give2, TestRole::Something);

    if let Err(e) = run(cortex) {
        eprintln!("error {:#?}", e)
    }
    else {
        panic!("GiveSomethingLobe should not accept this input")
    }
}

#[test]
fn test_require_one() {
    // make sure require one works as intended
    {
        let mut cortex = Cortex::new(GiveSomethingLobe::new().unwrap());

        let give = cortex.get_main_handle();
        let take = cortex.add_lobe(TakeSomethingLobe::new().unwrap());

        cortex.connect(give, take, TestRole::Something);

        run(cortex).unwrap();
    }

    // make sure require one fails as intended
    {
        if let Err(e) = run(TakeSomethingLobe::new().unwrap()) {
            eprintln!("error {:#?}", e)
        }
        else {
            panic!("TakeSomethingLobe has no input, so it should fail")
        }

        if let Err(e) = run(GiveSomethingLobe::new().unwrap()) {
            eprintln!("error {:#?}", e)
        }
        else {
            panic!("GiveSomethingLobe has no output, so it should fail")
        }
    }
}
