use std::{
    pin::Pin,
    task::{Context, Poll, ready},
    time::Instant,
};

use http_body::{Body, Frame};
use opentelemetry::KeyValue;
use pin_project::{pin_project, pinned_drop};
use tracing::Span;
use opentelemetry::trace::TraceContextExt as _;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;
use uuid::Uuid;

use super::{layer::ActiveRequestGuard, request_span};
use crate::app::AppMetrics;
use crate::utils::request_logging::request_id::RequestId;
use crate::utils::request_logging::access_log::AccessLogContext;

#[pin_project(PinnedDrop)]
pub struct TraceResponseBody {
    #[pin]
    pub(crate) inner: axum::body::Body,
    pub(crate) request_start: Instant,
    pub(crate) last_polled_at: Instant,
    pub(crate) span: Span,
    pub(crate) metrics: AppMetrics,
    pub(crate) attributes: Vec<KeyValue>,
    pub(crate) active_request: Option<ActiveRequestGuard>,
    pub(crate) total_bytes: u64,
    pub(crate) status_code: Option<u16>,
    pub(crate) http_route: String,
    pub(crate) http_method: String,
    pub(crate) access_log: AccessLogContext,
    pub(crate) access_logged: bool,
    pub(crate) request_id: RequestId,
}

impl Body for TraceResponseBody {
    type Data = axum::body::Bytes;
    type Error = axum::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = self.project();
        let _guard = this.span.enter();
        let result = ready!(this.inner.poll_frame(cx));

        *this.last_polled_at = Instant::now();

        match result {
            Some(Ok(frame)) => {
                let frame = match frame.into_data() {
                    Ok(chunk) => {
                        *this.total_bytes += chunk.len() as u64;
                        Frame::data(chunk)
                    }
                    Err(frame) => frame,
                };

                let frame = match frame.into_trailers() {
                    Ok(trailers) => Frame::trailers(trailers),
                    Err(frame) => frame,
                };

                Poll::Ready(Some(Ok(frame)))
            }
            Some(Err(err)) => Poll::Ready(Some(Err(err))),
            None => {
                emit_access_log(
                    this.span,
                    *this.request_start,
                    *this.request_id,
                    this.http_method,
                    this.http_route,
                    this.status_code.map(i64::from),
                    Some(*this.total_bytes),
                    this.access_log,
                );
                *this.access_logged = true;
                this.metrics
                    .response_size_bytes
                    .record(*this.total_bytes, this.attributes);
                request_span::on_end_of_stream(*this.total_bytes, this.span);
                if let Some(mut guard) = this.active_request.take() {
                    guard.finish();
                }
                Poll::Ready(None)
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

#[pinned_drop]
impl PinnedDrop for TraceResponseBody {
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();
        if !*this.access_logged {
            emit_access_log(
                this.span,
                *this.request_start,
                *this.request_id,
                this.http_method,
                this.http_route,
                this.status_code.map(i64::from),
                Some(*this.total_bytes),
                this.access_log,
            );
            *this.access_logged = true;
        }
        if let Some(mut guard) = this.active_request.take() {
            guard.finish();
        }
    }
}

pub(crate) fn emit_access_log(
    span: &Span,
    request_start: Instant,
    request_id: RequestId,
    http_method: &str,
    http_route: &str,
    status_code: Option<i64>,
    response_body_size: Option<u64>,
    ctx: &AccessLogContext,
) {
    let duration_ms = request_start.elapsed().as_millis() as i64;
    let trace_id = span
        .context()
        .span()
        .span_context()
        .trace_id()
        .to_string();
    let status_code = status_code.unwrap_or(500);
    let snapshot = ctx.snapshot();

    let repository_id: Option<Uuid> = snapshot.repository_id;
    let user = snapshot.user;

    match (repository_id, user) {
        (Some(repository_id), Some(user)) => {
            tracing::info!(
                target: "nitro_repo::access",
                trace_id = %trace_id,
                http.request.method = %http_method,
                http.route = %http_route,
                http.response.status_code = status_code,
                duration_ms = duration_ms,
                request_id = %request_id,
                repository_id = %repository_id,
                user = %user,
                http.response.body.size = response_body_size,
                "HTTP access"
            );
        }
        (Some(repository_id), None) => {
            tracing::info!(
                target: "nitro_repo::access",
                trace_id = %trace_id,
                http.request.method = %http_method,
                http.route = %http_route,
                http.response.status_code = status_code,
                duration_ms = duration_ms,
                request_id = %request_id,
                repository_id = %repository_id,
                http.response.body.size = response_body_size,
                "HTTP access"
            );
        }
        (None, Some(user)) => {
            tracing::info!(
                target: "nitro_repo::access",
                trace_id = %trace_id,
                http.request.method = %http_method,
                http.route = %http_route,
                http.response.status_code = status_code,
                duration_ms = duration_ms,
                request_id = %request_id,
                user = %user,
                http.response.body.size = response_body_size,
                "HTTP access"
            );
        }
        (None, None) => {
            tracing::info!(
                target: "nitro_repo::access",
                trace_id = %trace_id,
                http.request.method = %http_method,
                http.route = %http_route,
                http.response.status_code = status_code,
                duration_ms = duration_ms,
                request_id = %request_id,
                http.response.body.size = response_body_size,
                "HTTP access"
            );
        }
    }
}
