pub mod message {
    pub mod system {
        pub mod info {
            use chrono::{DateTime, Utc};
            use serde::{Deserialize, Serialize};

            pub struct Method; // unit-like struct
            impl super::super::super::super::method::RpcMethod for Method {
                const NAME: &'static str = "system.info";

                type RequestDto = SystemInfoRequestDto;
                type ResponseSuccessDto = SystemInfoResponseSuccessDto;
                type ResponseErrorDto = SystemInfoResponseErrorDto;
            }

            #[derive(Debug, Clone, Serialize, Deserialize)]
            pub struct SystemInfoRequestDto;

            #[derive(Debug, Clone, Serialize, Deserialize)]
            pub struct SystemInfoResponseSuccessDto {
                pub server_time: DateTime<Utc>,
                pub version: String,
                pub service_name: String,
            }

            #[derive(Debug, Clone, Serialize, Deserialize)]
            pub struct SystemInfoResponseErrorDto;
        }
    }
}

pub mod dtos {
    //
}
