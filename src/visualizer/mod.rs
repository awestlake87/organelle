use std::net::SocketAddr;

use futures::future;
use futures::prelude::*;
use futures::unsync::mpsc;
use hyper::{self, header};
use hyper::server::{Http, Service};
use hyper_staticfile::Static;
use open;
use serde_json;
use tokio_core::reactor;

use super::{Error, Result};
use axon::{Axon, Constraint};
use organelle::Organelle;
use probe::{self, Synapse, Terminal};
use soma::{self, Impulse};

pub struct Soma {
    probe: Option<Terminal>,
}

impl Soma {
    pub fn axon() -> Result<Axon<Self>> {
        Ok(Axon::new(
            Self { probe: None },
            vec![],
            vec![Constraint::One(Synapse::Probe)],
        ))
    }

    pub fn organelle(handle: reactor::Handle) -> Result<Organelle<Axon<Self>>> {
        let mut organelle = Organelle::new(Self::axon()?, handle);

        let visualizer = organelle.nucleus();
        let probe_soma = organelle.add_soma(probe::Soma::axon());

        organelle.connect(visualizer, probe_soma, Synapse::Probe)?;

        Ok(organelle)
    }
}

impl soma::Soma for Soma {
    type Synapse = Synapse;
    type Error = Error;

    #[async(boxed)]
    fn update(mut self, imp: Impulse<Self::Synapse>) -> Result<Self> {
        match imp {
            Impulse::AddTerminal(_, Synapse::Probe, tx) => {
                self.probe = Some(tx);
                Ok(self)
            },

            Impulse::Start(_, main_tx, handle) => {
                handle.spawn(
                    VisualizerTask::new(
                        self.probe.unwrap(),
                        main_tx.clone(),
                        handle.clone(),
                    ).run()
                        .or_else(move |e| {
                            main_tx
                                .send(Impulse::Error(e))
                                .map(|_| ())
                                .map_err(|_| ())
                        }),
                );

                Ok(Self { probe: None })
            },

            _ => bail!("unexpected impulse {:?}", imp),
        }
    }
}

struct VisualizerTask {
    probe: Terminal,
    port: u16,
    open_on_start: bool,
    main_tx: mpsc::Sender<Impulse<Synapse>>,
    handle: reactor::Handle,
}

impl VisualizerTask {
    fn new(
        probe: Terminal,
        main_tx: mpsc::Sender<Impulse<Synapse>>,
        handle: reactor::Handle,
    ) -> Self {
        Self {
            probe: probe,
            port: 8080,
            open_on_start: true,

            main_tx: main_tx,
            handle: handle,
        }
    }

    #[async]
    fn run(self) -> Result<()> {
        let addr: SocketAddr = format!("127.0.0.1:{}", self.port).parse()?;
        let stream_handle = self.handle.clone();
        let hypersf_handle = self.handle.clone();
        let main_tx = self.main_tx.clone();
        let probe = self.probe;

        if self.open_on_start {
            if let Err(e) = open::that(format!("http://{}", addr.to_string())) {
                eprintln!("unable to open default browser: {:#?}", e)
            }
        }

        await!(
            Http::new()
                .serve_addr_handle(&addr, &self.handle, move || Ok(
                    VisualizerService::new(&hypersf_handle, probe.clone())
                ))?
                .for_each(move |connection| {
                    stream_handle.spawn(connection.map(|_| ()).or_else(
                        move |e| {
                            eprintln!(
                                "error while serving HTTP request - {:?}",
                                e
                            );

                            Ok(())
                        },
                    ));

                    Ok(())
                })
        )?;

        Ok(())
    }
}

struct VisualizerService {
    ui: Static,
    probe: Terminal,
}

impl VisualizerService {
    fn new(handle: &reactor::Handle, probe: Terminal) -> Self {
        Self {
            ui: Static::new(handle, "src/visualizer"),
            probe: probe,
        }
    }

    fn get(&self, req: hyper::Request) -> <Self as Service>::Future {
        match req.path() {
            "/" => Box::new(self.ui.call(req)),
            _ => Box::new(
                Self::get_api(req, self.probe.clone()).map_err(|e| e.into()),
            ),
        }
    }

    #[async]
    fn get_api(
        req: hyper::Request,
        probe: Terminal,
    ) -> Result<hyper::Response> {
        if req.path() == "/api/probe/json" {
            await!(Self::probe_json(probe))
        } else {
            await!(Self::not_found(req))
        }
    }

    #[async]
    fn probe_json(probe: Terminal) -> Result<hyper::Response> {
        let mut rsp = hyper::Response::new();

        match await!(probe.probe()) {
            Ok(data) => {
                rsp.set_body(serde_json::to_string(&data)?);
            },
            Err(e) => {
                rsp.set_status(hyper::StatusCode::InternalServerError);
                rsp.set_body(format!("{:#?}", e));
            },
        }

        Ok(rsp)
    }

    #[async]
    fn not_found(req: hyper::Request) -> Result<hyper::Response> {
        let mut rsp = hyper::Response::new();
        rsp.set_status(hyper::StatusCode::NotFound);
        rsp.set_body(format!("Error 404 {} Not Found", req.uri()));

        Ok(rsp)
    }
}

impl Service for VisualizerService {
    type Request = hyper::Request;
    type Response = hyper::Response;
    type Error = hyper::Error;
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, req: hyper::Request) -> Self::Future {
        match req.method() {
            &hyper::Method::Get => self.get(req),

            _ => Box::new(Self::not_found(req).map_err(|e| e.into())),
        }
    }
}
