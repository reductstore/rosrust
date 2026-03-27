use serde::{Deserialize, Serialize};
use std;
use std::collections::HashMap;
use tiny_http::{self, Header};

use super::error::{ErrorKind, Result};
use super::xmlfmt::{error, from_params, into_params, parse, Call, Fault, Response, Value};

type Handler = Box<dyn Fn(Vec<Value>) -> Response + Send + Sync>;
type HandlerMap = HashMap<String, Handler>;

pub fn on_decode_fail(err: &error::Error) -> Response {
    Err(Fault::new(
        400,
        format!("Failed to decode request: {}", err),
    ))
}

pub fn on_encode_fail(err: &error::Error) -> Response {
    Err(Fault::new(
        500,
        format!("Failed to encode response: {}", err),
    ))
}

fn on_missing_method(_: Vec<Value>) -> Response {
    Err(Fault::new(404, "Requested method does not exist"))
}

pub struct Server {
    handlers: HandlerMap,
    on_missing_method: Handler,
}

impl Default for Server {
    fn default() -> Self {
        Server {
            handlers: HashMap::new(),
            on_missing_method: Box::new(on_missing_method),
        }
    }
}

impl Server {
    pub fn new() -> Server {
        Server::default()
    }

    pub fn register_value<K, T>(&mut self, name: K, handler: T)
    where
        K: Into<String>,
        T: Fn(Vec<Value>) -> Response + Send + Sync + 'static,
    {
        self.handlers.insert(name.into(), Box::new(handler));
    }

    pub fn register<'a, K, Treq, Tres, Thandler, Tef, Tdf>(
        &mut self,
        name: K,
        handler: Thandler,
        encode_fail: Tef,
        decode_fail: Tdf,
    ) where
        K: Into<String>,
        Treq: Deserialize<'a>,
        Tres: Serialize,
        Thandler: Fn(Treq) -> std::result::Result<Tres, Fault> + Send + Sync + 'static,
        Tef: Fn(&error::Error) -> Response + Send + Sync + 'static,
        Tdf: Fn(&error::Error) -> Response + Send + Sync + 'static,
    {
        self.register_value(name, move |req| {
            let params = match from_params(req) {
                Ok(v) => v,
                Err(err) => return decode_fail(&err),
            };
            let response = handler(params)?;
            into_params(&response).or_else(|v| encode_fail(&v))
        });
    }

    pub fn register_simple<'a, K, Treq, Tres, Thandler>(&mut self, name: K, handler: Thandler)
    where
        K: Into<String>,
        Treq: Deserialize<'a>,
        Tres: Serialize,
        Thandler: Fn(Treq) -> std::result::Result<Tres, Fault> + Send + Sync + 'static,
    {
        self.register(name, handler, on_encode_fail, on_decode_fail);
    }

    pub fn set_on_missing<T>(&mut self, handler: T)
    where
        T: Fn(Vec<Value>) -> Response + Send + Sync + 'static,
    {
        self.on_missing_method = Box::new(handler);
    }

    pub fn bind(
        self,
        uri: &std::net::SocketAddr,
    ) -> Result<BoundServer>
    {
        tiny_http::Server::http(uri)
            .map_err(|err| ErrorKind::BindFail(err.to_string()).into())
            .map(|server| BoundServer::new(server, self.handlers, self.on_missing_method))
    }
}

pub struct BoundServer {
    server: tiny_http::Server,
    handlers: HandlerMap,
    on_missing_method: Handler,
}

impl BoundServer {
    fn new(server: tiny_http::Server, handlers: HandlerMap, on_missing_method: Handler) -> Self {
        Self {
            server,
            handlers,
            on_missing_method,
        }
    }

    pub fn local_addr(&self) -> std::net::SocketAddr {
        match self.server.server_addr() {
            tiny_http::ListenAddr::IP(addr) => addr,
            #[cfg(unix)]
            tiny_http::ListenAddr::Unix(addr) => {
                panic!("Expected IP listen address, got unix socket {:?}", addr)
            }
        }
    }

    pub fn run(self) {
        for mut request in self.server.incoming_requests() {
            let response = self.handle_outer(&mut request);
            let _ = request.respond(response);
        }
    }

    pub fn poll(&self) {
        loop {
            let request = match self.server.try_recv() {
                Ok(Some(request)) => request,
                Ok(None) | Err(_) => break,
            };
            let mut request = request;
            let response = self.handle_outer(&mut request);
            let _ = request.respond(response);
        }
    }

    fn handle_outer(&self, request: &mut tiny_http::Request) -> tiny_http::ResponseBox {
        use super::xmlfmt::value::ToXml;

        let call: Call = match parse::call(request.as_reader()) {
            Ok(data) => data,
            Err(_) => return tiny_http::Response::empty(400).boxed(),
        };

        let res = self.handle(call);
        let body = res.to_xml();
        let mut response = tiny_http::Response::from_data(body.into_bytes());
        if let Ok(content_type) = Header::from_bytes(&b"Content-Type"[..], &b"text/xml"[..]) {
            response.add_header(content_type);
        }
        response.boxed()
    }

    fn handle(&self, req: Call) -> Response {
        self.handlers
            .get(&req.name)
            .unwrap_or(&self.on_missing_method)(req.params)
    }
}
