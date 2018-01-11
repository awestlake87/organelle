
#[macro_use]
extern crate error_chain;

extern crate organelle;

use organelle::*;

enum TestMessage {
    Something,
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
enum TestRole {
    Something,
}

struct GiveSomethingCell {
    soma:           Soma<TestMessage, TestRole>,
}

impl GiveSomethingCell {
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

impl Cell for GiveSomethingCell {
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

struct TakeSomethingCell {
    soma:           Soma<TestMessage, TestRole>,
}

impl TakeSomethingCell {
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

impl Cell for TakeSomethingCell {
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
    let mut organelle = Organelle::new(GiveSomethingCell::new().unwrap());

    let give1 = organelle.get_main_handle();
    let give2 = organelle.add_cell(GiveSomethingCell::new().unwrap());

    organelle.connect(give1, give2, TestRole::Something);

    if let Err(e) = run(organelle) {
        eprintln!("error {:#?}", e)
    }
    else {
        panic!("GiveSomethingCell should not accept this input")
    }
}

#[test]
fn test_require_one() {
    // make sure require one works as intended
    {
        let mut organelle = Organelle::new(GiveSomethingCell::new().unwrap());

        let give = organelle.get_main_handle();
        let take = organelle.add_cell(TakeSomethingCell::new().unwrap());

        organelle.connect(give, take, TestRole::Something);

        run(organelle).unwrap();
    }

    // make sure require one fails as intended
    {
        if let Err(e) = run(TakeSomethingCell::new().unwrap()) {
            eprintln!("error {:#?}", e)
        }
        else {
            panic!("TakeSomethingCell has no input, so it should fail")
        }

        if let Err(e) = run(GiveSomethingCell::new().unwrap()) {
            eprintln!("error {:#?}", e)
        }
        else {
            panic!("GiveSomethingCell has no output, so it should fail")
        }
    }
}
