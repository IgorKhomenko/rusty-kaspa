// pub type RpcHeader = kaspa_consensus_core::header::Header;

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use kaspa_consensus_core::{header::Header, BlueWorkType};
use kaspa_hashes::Hash;
use kaspa_math::Uint192;
use kaspa_utils::hex::*;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::to_value;
use wasm_bindgen::prelude::*;
use workflow_wasm::jsvalue::*;

#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize, BorshSchema)]
#[serde(rename_all = "camelCase")]
#[wasm_bindgen(inspectable)]
pub struct RpcHeader {
    #[wasm_bindgen(skip)]
    pub hash: Hash, // Cached hash
    pub version: u16,
    #[wasm_bindgen(skip)]
    pub parents_by_level: Vec<Vec<Hash>>,
    pub hash_merkle_root: Hash,
    pub accepted_id_merkle_root: Hash,
    pub utxo_commitment: Hash,
    pub timestamp: u64, // Timestamp is in milliseconds
    pub bits: u32,
    pub nonce: u64,
    pub daa_score: u64,
    #[serde(with = "kaspa_utils::hex")]
    #[wasm_bindgen(skip)]
    pub blue_work: BlueWorkType,
    pub blue_score: u64,
    pub pruning_point: Hash,
}

#[wasm_bindgen]
impl RpcHeader {
    #[wasm_bindgen(getter = hash)]
    pub fn get_hash_as_hex(&self) -> String {
        self.hash.to_hex()
    }

    #[wasm_bindgen(setter = hash)]
    pub fn set_hash_from_js_value(&mut self, js_value: JsValue) {
        self.hash = Hash::from_slice(&js_value.try_as_vec_u8().expect("invalid hash"));
    }

    #[wasm_bindgen(getter = parentsByLevel)]
    pub fn get_parents_by_level_as_js_value(&self) -> JsValue {
        to_value(&self.parents_by_level).expect("invalid parents_by_level")
    }

    #[wasm_bindgen(getter = blueWork)]
    pub fn get_blue_work_as_hex(&self) -> String {
        (&self.blue_work).to_hex()
    }

    #[wasm_bindgen(setter = blueWork)]
    pub fn set_blue_work_from_js_value(&mut self, js_value: JsValue) {
        let vec = js_value.try_as_vec_u8().expect("invalid blue work");
        self.blue_work = Uint192::from_be_bytes(vec.as_slice().try_into().expect("invalid byte length"));
    }
}

impl RpcHeader {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        version: u16,
        parents_by_level: Vec<Vec<Hash>>,
        hash_merkle_root: Hash,
        accepted_id_merkle_root: Hash,
        utxo_commitment: Hash,
        timestamp: u64,
        bits: u32,
        nonce: u64,
        daa_score: u64,
        blue_work: BlueWorkType,
        blue_score: u64,
        pruning_point: Hash,
    ) -> Self {
        let header = Header::new(
            version,
            parents_by_level,
            hash_merkle_root,
            accepted_id_merkle_root,
            utxo_commitment,
            timestamp,
            bits,
            nonce,
            daa_score,
            blue_work,
            blue_score,
            pruning_point,
        );
        // header.finalize();
        (&header).into()
    }

    // TODO - review conversion handling and remove code below if not needed.

    // Finalizes the header and recomputes the header hash
    // pub fn finalize(&mut self) {
    //     self.hash = hashing::header::hash(self);
    // }

    // pub fn direct_parents(&self) -> &[Hash] {
    //     if self.parents_by_level.is_empty() {
    //         &[]
    //     } else {
    //         &self.parents_by_level[0]
    //     }
    // }
}

impl From<&Header> for RpcHeader {
    fn from(header: &Header) -> Self {
        Self {
            hash: header.hash,
            version: header.version,
            parents_by_level: header.parents_by_level.clone(),
            hash_merkle_root: header.hash_merkle_root,
            accepted_id_merkle_root: header.accepted_id_merkle_root,
            utxo_commitment: header.utxo_commitment,
            timestamp: header.timestamp,
            bits: header.bits,
            nonce: header.nonce,
            daa_score: header.daa_score,
            blue_work: header.blue_work,
            blue_score: header.blue_score,
            pruning_point: header.pruning_point,
        }
    }
}

impl From<&RpcHeader> for Header {
    fn from(header: &RpcHeader) -> Self {
        Self {
            hash: header.hash,
            version: header.version,
            parents_by_level: header.parents_by_level.clone(),
            hash_merkle_root: header.hash_merkle_root,
            accepted_id_merkle_root: header.accepted_id_merkle_root,
            utxo_commitment: header.utxo_commitment,
            timestamp: header.timestamp,
            bits: header.bits,
            nonce: header.nonce,
            daa_score: header.daa_score,
            blue_work: header.blue_work,
            blue_score: header.blue_score,
            pruning_point: header.pruning_point,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaspa_math::Uint192;
    use serde_json::Value;

    #[test]
    fn test_rpc_header() {
        let header = RpcHeader::new(
            1,
            vec![vec![1.into()]],
            Default::default(),
            Default::default(),
            Default::default(),
            234,
            23,
            567,
            0,
            Uint192([0x1234567890abcfed, 0xc0dec0ffeec0ffee, 0x1234567890abcdef]),
            u64::MAX,
            Default::default(),
        );
        let json = serde_json::to_string(&header).unwrap();
        println!("{}", json);

        let v = serde_json::from_str::<Value>(&json).unwrap();
        let blue_work = v.get("blueWork").expect("missing `blueWork` property");
        let blue_work = blue_work.as_str().expect("`blueWork` is not a string");
        assert_eq!(blue_work, "1234567890abcdefc0dec0ffeec0ffee1234567890abcfed");
        let blue_score = v.get("blueScore").expect("missing `blueScore` property");
        let blue_score: u64 = blue_score.as_u64().expect("blueScore is not a u64 compatible value");
        assert_eq!(blue_score, u64::MAX);

        let h = serde_json::from_str::<RpcHeader>(&json).unwrap();
        assert!(h.blue_score == header.blue_score && h.blue_work == header.blue_work);
    }
}
