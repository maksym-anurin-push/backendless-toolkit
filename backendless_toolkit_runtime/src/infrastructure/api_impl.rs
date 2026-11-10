use async_trait::async_trait;
use chrono::Utc;
use backendless_toolkit_contract::{Api, InvokerError, protocol::message};

use crate::application::{
    UseCasesBundle,
};

pub struct ApiImpl {
    use_cases: UseCasesBundle,
}

impl ApiImpl {
    pub fn new(use_cases: UseCasesBundle) -> Self {
        Self { use_cases }
    }
}

#[async_trait]
impl Api for ApiImpl {
    async fn system_info(
        &self,
        _req: message::system::info::SystemInfoRequestDto,
    ) -> Result<
        message::system::info::SystemInfoResponseSuccessDto,
        InvokerError<message::system::info::SystemInfoResponseErrorDto>,
    > {
        let version: &String = &self.use_cases.app_info.version;
        let service_name: &String = &self.use_cases.app_info.name;

        Ok(message::system::info::SystemInfoResponseSuccessDto {
            server_time: Utc::now(),
            version: version.clone(),
            service_name: service_name.clone(),
        })
    }
}
