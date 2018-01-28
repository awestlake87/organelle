#[allow(dead_code)]
mod dot;

use std::collections::HashMap;
use std::net::SocketAddr;

use bytes::BufMut;
use futures::future;
use futures::prelude::*;
use hyper;
use hyper::server::{Http, Service};
use open;
use serde_json;
use tokio_core::reactor;
use uuid::Uuid;

use super::{Error, Result};
use axon::{Axon, Constraint};
use organelle::Organelle;
use probe::{self, ConstraintData, SomaData, Synapse, Terminal};
use soma::{self, Impulse};

/// visualizer settings
#[derive(Debug, Clone)]
pub struct Settings {
    open_on_start: bool,
    port: u16,
}

impl Settings {
    /// open a browser with the visualizer upon starting up
    pub fn open_on_start(self, flag: bool) -> Self {
        Self {
            open_on_start: flag,
            ..self
        }
    }

    /// set the port that the visualizer is hosted on
    pub fn port(self, port: u16) -> Self {
        Self { port: port, ..self }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            open_on_start: false,
            port: 8080,
        }
    }
}

/// soma that hosts a service and a ui that can be viewed in a browser
pub struct Soma {
    settings: Settings,
    probe: Option<Terminal>,
}

impl Soma {
    /// create a visualizer to use with another probe soma
    pub fn axon(settings: Settings) -> Result<Axon<Self>> {
        Ok(Axon::new(
            Self {
                settings: settings,
                probe: None,
            },
            vec![],
            vec![Constraint::One(Synapse::Probe)],
        ))
    }

    /// create a standalone organelle to plug into any system
    pub fn organelle(
        settings: Settings,
        handle: reactor::Handle,
    ) -> Result<Organelle<Axon<Self>>> {
        let mut organelle = Organelle::new(Self::axon(settings)?, handle);

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
                        self.settings.clone(),
                        self.probe.unwrap(),
                        handle.clone(),
                    ).run()
                        .or_else(move |e| {
                            main_tx
                                .send(Impulse::Error(e))
                                .map(|_| ())
                                .map_err(|_| ())
                        }),
                );

                Ok(Self {
                    settings: self.settings,
                    probe: None,
                })
            },

            _ => bail!("unexpected impulse {:?}", imp),
        }
    }
}

struct VisualizerTask {
    probe: Terminal,
    port: u16,
    open_on_start: bool,
    handle: reactor::Handle,
}

impl VisualizerTask {
    fn new(
        settings: Settings,
        probe: Terminal,
        handle: reactor::Handle,
    ) -> Self {
        Self {
            probe: probe,
            port: settings.port,
            open_on_start: settings.open_on_start,

            handle: handle,
        }
    }

    #[async]
    fn run(self) -> Result<()> {
        let addr: SocketAddr = format!("127.0.0.1:{}", self.port).parse()?;
        let stream_handle = self.handle.clone();
        let hypersf_handle = self.handle.clone();
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
    probe: Terminal,
}

impl VisualizerService {
    fn new(_handle: &reactor::Handle, probe: Terminal) -> Self {
        Self { probe: probe }
    }

    fn get(&self, req: hyper::Request) -> <Self as Service>::Future {
        match req.path() {
            "/" | "/index.html" => {
                let mut rsp = hyper::Response::new();

                rsp.set_body(include_str!("index.html"));

                Box::new(future::ok(rsp))
            },
            "/viz-lite.js" => {
                let mut rsp = hyper::Response::new();

                rsp.set_body(include_str!("viz-lite.js"));

                Box::new(future::ok(rsp))
            },
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
        } else if req.path() == "/api/probe/dot" {
            await!(Self::probe_dot(probe))
        } else {
            await!(Self::not_found(req))
        }
    }

    #[async]
    fn probe_json(probe: Terminal) -> Result<hyper::Response> {
        let mut rsp = hyper::Response::new();

        match await!(probe.probe(probe::Settings::new())) {
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
    fn probe_dot(probe: Terminal) -> Result<hyper::Response> {
        let mut rsp = hyper::Response::new();

        match await!(probe.probe(probe::Settings::new())) {
            Ok(data) => {
                rsp.set_body(render_dot(data)?);
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

fn render_organelle(
    uuid: Uuid,
    name: String,
    nucleus: SomaData,
    mut somas: Vec<SomaData>,
    remap: &HashMap<Uuid, Uuid>,
) -> dot::SubGraph {
    let mut organelle = dot::SubGraph::new()
        .id(dot::Id::quoted(format!("cluster_{}", uuid)))
        .add(
            dot::Selector::graph()
                .add(dot::Attribute::new(
                    dot::Id::ident("style"),
                    dot::Id::ident("rounded"),
                ))
                .add(dot::Attribute::new(
                    dot::Id::ident("label"),
                    dot::Id::quoted(name),
                )),
        );

    let mut edges = vec![];

    somas.push(nucleus);

    for soma in somas {
        match &soma {
            &SomaData::Axon {
                uuid,
                ref terminals,
                ..
            } => {
                let src_uuid = uuid;

                for t in terminals {
                    match t {
                        &ConstraintData::One { ref variant, soma } => {
                            let tgt_uuid = if let Some(uuid) = remap.get(&soma)
                            {
                                *uuid
                            } else {
                                soma
                            };

                            edges.push(dot::NodeId::new(dot::Id::quoted(
                                src_uuid.to_string(),
                            )).port(dot::Id::ident(format!("t_{}", variant)))
                                .connect(
                                    dot::EdgeOp::Directed,
                                    dot::NodeId::new(dot::Id::quoted(
                                        tgt_uuid.to_string(),
                                    )).port(dot::Id::ident(format!(
                                        "d_{}",
                                        variant
                                    ))),
                                ));
                        },
                        &ConstraintData::Variadic {
                            ref variant,
                            ref somas,
                        } => for uuid in somas {
                            let tgt_uuid =
                                if let Some(remapped) = remap.get(&uuid) {
                                    *remapped
                                } else {
                                    *uuid
                                };

                            edges.push(dot::NodeId::new(dot::Id::quoted(
                                src_uuid.to_string(),
                            )).port(dot::Id::ident(format!("t_{}", variant)))
                                .connect(
                                    dot::EdgeOp::Directed,
                                    dot::NodeId::new(dot::Id::quoted(
                                        tgt_uuid.to_string(),
                                    )).port(dot::Id::ident(format!(
                                        "d_{}",
                                        variant
                                    ))),
                                ));
                        },
                    }
                }
            },
            _ => (),
        }
        organelle = organelle.add(render_soma(soma, remap));
    }

    for edge in edges {
        organelle = organelle.add(edge);
    }

    organelle
}

fn render_axon(
    uuid: Uuid,
    name: String,
    terminals: Vec<ConstraintData>,
    dendrites: Vec<ConstraintData>,
    _remap: &HashMap<Uuid, Uuid>,
) -> dot::SubGraph {
    let mut axon = dot::SubGraph::new();

    let terminals: Vec<String> = terminals
        .into_iter()
        .map(|t| match t {
            ConstraintData::One { variant, .. } => {
                format!("<t_{}> {}", variant, variant)
            },
            ConstraintData::Variadic { variant, .. } => {
                format!("<t_{}> {}", variant, variant)
            },
        })
        .collect();

    let terminals = terminals.join(" | ");

    let dendrites: Vec<String> = dendrites
        .into_iter()
        .map(|d| match d {
            ConstraintData::One { variant, .. } => {
                format!("<d_{}> {}", variant, variant)
            },
            ConstraintData::Variadic { variant, .. } => {
                format!("<d_{}> {}", variant, variant)
            },
        })
        .collect();

    let dendrites = dendrites.join(" | ");

    axon = axon.add(
        dot::Node::new(dot::Id::quoted(uuid.to_string()))
            .add(dot::Attribute::new(
                dot::Id::ident("label"),
                dot::Id::quoted(format!(
                    "<name> {} | {{ {{ {} }} | {{ }} | {{ {} }} }} | {{ }}",
                    name.replace("<", "\\<").replace(">", "\\>"),
                    dendrites,
                    terminals,
                )),
            ))
            .add(dot::Attribute::new(
                dot::Id::ident("shape"),
                dot::Id::ident("Mrecord"),
            ))
            .add(dot::Attribute::new(
                dot::Id::ident("style"),
                dot::Id::ident("rounded"),
            )),
    );

    axon
}

fn render_soma(data: SomaData, remap: &HashMap<Uuid, Uuid>) -> dot::SubGraph {
    match data {
        SomaData::Organelle {
            uuid,
            nucleus,
            somas,
            name,
        } => render_organelle(uuid, name, *nucleus, somas, remap),
        SomaData::Axon {
            terminals,
            dendrites,
            uuid,
            name,
        } => render_axon(uuid, name, terminals, dendrites, remap),
        _ => unimplemented!(),
    }
}

fn get_uuid(data: &SomaData) -> Option<Uuid> {
    match data {
        &SomaData::Organelle { ref nucleus, .. } => get_uuid(nucleus),
        &SomaData::Axon { uuid, .. } => Some(uuid),
        _ => None,
    }
}

fn remap_uuids(data: &SomaData, remap: &mut HashMap<Uuid, Uuid>) {
    match data {
        &SomaData::Organelle {
            uuid,
            ref nucleus,
            ref somas,
            ..
        } => {
            if let Some(inner_uuid) = get_uuid(data) {
                remap.insert(uuid, inner_uuid);
            }

            remap_uuids(nucleus, remap);

            for soma in somas {
                remap_uuids(soma, remap);
            }
        },
        _ => (),
    }
}

fn render_dot(data: SomaData) -> Result<String> {
    let buf = Vec::new();
    let mut writer = buf.writer();

    let mut remap = HashMap::new();

    remap_uuids(&data, &mut remap);

    let dot = dot::Dot::DiGraph(
        dot::SubGraph::new().add(render_soma(data, &remap)).add(
            dot::Attribute::new(
                dot::Id::ident("rankdir"),
                dot::Id::ident("LR"),
            ),
        ),
    );

    dot.render(&mut writer)?;

    let viz = String::from_utf8(writer.into_inner())?;

    Ok(viz)
}
