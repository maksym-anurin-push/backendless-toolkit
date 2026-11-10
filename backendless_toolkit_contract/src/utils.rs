/// `Option<T>` that must be present in JSON (`null` or a value). Omitted field is an error.
pub mod required_nullable {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S, T>(value: &Option<T>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Serialize,
    {
        value.serialize(serializer)
    }

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        // Using deserialize_with disables serde's Option "missing field => None" special case.
        Option::<T>::deserialize(deserializer)
    }
}

pub mod bs58_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&bs58::encode(bytes).into_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let data: Vec<u8> = bs58::decode(s)
            .into_vec()
            .map_err(serde::de::Error::custom)?;
        let array: [u8; 32] = data
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid rpc identifier length"))?;
        Ok(array)
    }
}

// pub mod hex_serde {
//     use serde::{Deserialize, Deserializer, Serializer};

//     pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
//     where
//         S: Serializer,
//     {
//         serializer.serialize_str(&hex::encode(bytes))
//     }

//     pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
//     where
//         D: Deserializer<'de>,
//     {
//         let s = String::deserialize(deserializer)?;
//         let data: Vec<u8> = hex::decode(s).map_err(serde::de::Error::custom)?;
//         let array: [u8; 32] = data
//             .try_into()
//             .map_err(|_| serde::de::Error::custom("invalid rpc identifier length"))?;
//         Ok(array)
//     }
// }

// pub mod base64_serde {
//     use base64::{Engine, prelude::BASE64_STANDARD};
//     use serde::{Deserialize, Deserializer, Serializer};

//     pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
//     where
//         S: Serializer,
//     {
//         let s = BASE64_STANDARD.encode(bytes);
//         serializer.serialize_str(&s)
//     }

//     pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
//     where
//         D: Deserializer<'de>,
//     {
//         let s = String::deserialize(deserializer)?;
//         BASE64_STANDARD.decode(s).map_err(serde::de::Error::custom)
//     }
// }
