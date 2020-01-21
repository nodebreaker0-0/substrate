// Copyright 2019 Parity Technologies (UK) Ltd.
// This file is part of Substrate.

// Substrate is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Substrate is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Substrate.  If not, see <http://www.gnu.org/licenses/>.

use futures_util::{FutureExt, future::Future};
use hyper::http::StatusCode;
use hyper::{Server, Body, Response, service::{service_fn, make_service_fn}};
use prometheus::{Encoder, Opts, TextEncoder, core::Atomic};
use std::net::SocketAddr;
#[cfg(not(target_os = "unknown"))]
mod networking;

pub use prometheus::{
	Registry, Error as PrometheusError,
	core::{GenericGauge as Gauge, AtomicF64 as F64, AtomicI64 as I64, AtomicU64 as U64}
};

pub fn create_gauge<T: Atomic + 'static>(name: &str, description: &str, registry: &Registry) -> Result<Gauge<T>, PrometheusError> {
	let gauge = Gauge::with_opts(Opts::new(name, description))?;
	registry.register(Box::new(gauge.clone()))?;
	Ok(gauge)
}

#[derive(Debug, derive_more::Display, derive_more::From)]
pub enum Error {
	/// Hyper internal error.
	Hyper(hyper::Error),
	/// Http request error.
	Http(hyper::http::Error),
	/// i/o error.
	Io(std::io::Error),
	#[display(fmt = "Prometheus export port {} already in use.", _0)]
	PortInUse(SocketAddr)
}

impl std::error::Error for Error {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		match self {
			Error::Hyper(error) => Some(error),
			Error::Http(error) => Some(error),
			Error::Io(error) => Some(error),
			Error::PortInUse(_) => None
		}
	}
}

async fn request_metrics(registry: Registry) -> Result<Response<Body>, Error> {
	let metric_families = registry.gather();
	let mut buffer = vec![];
	let encoder = TextEncoder::new();
	encoder.encode(&metric_families, &mut buffer).unwrap();

	Response::builder()
		.status(StatusCode::OK)
		.header("Content-Type", encoder.format_type())
		.body(Body::from(buffer))
		.map_err(Error::Http)
}

#[derive(Clone)]
pub struct Executor;

#[cfg(not(target_os = "unknown"))]
impl<T> hyper::rt::Executor<T> for Executor
	where
		T: Future + Send + 'static,
		T::Output: Send + 'static,
{
	fn execute(&self, future: T) {
		async_std::task::spawn(future);
	}
}
/// Initializes the metrics context, and starts an HTTP server
/// to serve metrics.
#[cfg(not(target_os = "unknown"))]
pub async fn init_prometheus(prometheus_addr: SocketAddr, registry: Registry) -> Result<(), Error>{
	use networking::Incoming;
	let listener = async_std::net::TcpListener::bind(&prometheus_addr)
		.await
		.map_err(|_| Error::PortInUse(prometheus_addr))?;

	log::info!("Prometheus server started at {}", prometheus_addr);

	let service = make_service_fn(move |_| {
		let registry = registry.clone();

		async move {
			Ok::<_, hyper::Error>(service_fn(move |_| {
				request_metrics(registry.clone())
			}))
		}
	});

	let server = Server::builder(Incoming(listener.incoming()))
		.executor(Executor)
		.serve(service)
		.boxed();

	let result = server.await.map_err(Into::into);

	result
}

#[cfg(target_os = "unknown")]
pub async fn init_prometheus(_: SocketAddr) -> Result<(), Error> {
	Ok(())
}
