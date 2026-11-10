use async_trait::async_trait;
use freemework_abstractions::sync::FDisposable;

pub use super::method::RpcMethod;

pub enum InvokerError<ResponseErrorDto> {
    Server(ResponseErrorDto),
    Transport(String),
}
impl<TResponseError: std::fmt::Display> std::fmt::Display for InvokerError<TResponseError> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Server(error) => write!(f, "Server error: {error}"),
            Self::Transport(error) => write!(f, "Transport error: {error}"),
        }
    }
}

#[async_trait]
pub trait Invoker: FDisposable + core::marker::Send + core::marker::Sync {
    async fn invoke<M: RpcMethod>(
        &self,
        proto_req: M::RequestDto,
    ) -> core::result::Result<M::ResponseSuccessDto, InvokerError<M::ResponseErrorDto>>;
}

impl<T> std::fmt::Debug for InvokerError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Server(_) => f
                .debug_tuple("Server")
                .field(&"<Server Error>".to_string())
                .finish(),
            Self::Transport(msg) => f.debug_tuple("Transport").field(msg).finish(),
        }
    }
}
