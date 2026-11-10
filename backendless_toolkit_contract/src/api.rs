use async_trait::async_trait;

use super::InvokerError;
use super::protocol::message;

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
pub trait Api: core::marker::Send + core::marker::Sync {
    async fn system_info(
        &self,
        req: message::system::info::SystemInfoRequestDto,
    ) -> Result<
        message::system::info::SystemInfoResponseSuccessDto,
        InvokerError<message::system::info::SystemInfoResponseErrorDto>,
    >;
}
