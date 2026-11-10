pub trait RpcMethod: Send + Sync {
    type RequestDto: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de>;
    type ResponseSuccessDto: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de>;
    type ResponseErrorDto: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de>;

    const NAME: &'static str;
}
