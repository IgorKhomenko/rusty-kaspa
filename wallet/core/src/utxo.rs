use crate::imports::*;
use crate::result::Result;
use crate::tx::TransactionOutpoint;
use itertools::Itertools;
use kaspa_rpc_core::{GetUtxosByAddressesResponse, RpcUtxosByAddressesEntry};
use serde_wasm_bindgen::from_value;
use std::sync::atomic::{AtomicU32, Ordering};
use workflow_wasm::abi::{ref_from_abi, TryFromJsValue};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[wasm_bindgen(inspectable)]
pub struct UtxoEntry {
    #[wasm_bindgen(getter_with_clone)]
    pub address: Option<Address>,
    #[wasm_bindgen(getter_with_clone)]
    pub outpoint: TransactionOutpoint,
    #[wasm_bindgen(js_name=entry, getter_with_clone)]
    pub utxo_entry: cctx::UtxoEntry,
}

impl UtxoEntry {
    #[inline(always)]
    pub fn amount(&self) -> u64 {
        self.utxo_entry.amount
    }
    #[inline(always)]
    pub fn block_daa_score(&self) -> u64 {
        self.utxo_entry.block_daa_score
    }
}

impl From<RpcUtxosByAddressesEntry> for UtxoEntry {
    fn from(entry: RpcUtxosByAddressesEntry) -> UtxoEntry {
        UtxoEntry { address: entry.address, outpoint: entry.outpoint.try_into().unwrap(), utxo_entry: entry.utxo_entry }
    }
}

#[derive(Clone, TryFromJsValue)]
#[wasm_bindgen(inspectable)]
pub struct UtxoEntryReference {
    #[wasm_bindgen(skip)]
    pub utxo: Arc<UtxoEntry>,
}

#[wasm_bindgen]
impl UtxoEntryReference {
    #[wasm_bindgen(getter)]
    pub fn data(&self) -> UtxoEntry {
        self.as_ref().clone()
    }
}

impl AsRef<UtxoEntry> for UtxoEntryReference {
    fn as_ref(&self) -> &UtxoEntry {
        &self.utxo
    }
}

impl From<UtxoEntryReference> for UtxoEntry {
    fn from(value: UtxoEntryReference) -> Self {
        (*value.utxo).clone()
    }
}

impl From<RpcUtxosByAddressesEntry> for UtxoEntryReference {
    fn from(entry: RpcUtxosByAddressesEntry) -> Self {
        Self { utxo: Arc::new(entry.into()) }
    }
}

#[wasm_bindgen]
/// Result containing data produced by the `UtxoSet::select()` function
pub struct SelectionContext {
    #[wasm_bindgen(js_name = "amount")]
    pub transaction_amount: u64,
    #[wasm_bindgen(js_name = "totalAmount")]
    pub total_selected_amount: u64,
    #[wasm_bindgen(skip)]
    pub selected_entries: Vec<UtxoEntryReference>,
}

#[wasm_bindgen]
impl SelectionContext {
    #[wasm_bindgen(getter=utxos)]
    pub fn selected_entries(&self) -> js_sys::Array {
        js_sys::Array::from_iter(self.selected_entries.clone().into_iter().map(JsValue::from))
    }
}

/// UtxoOrdering enum denotes UTXO sort order (`Unordered`, `AscendingAmount`, `AscendingDaaScore`)
#[derive(Clone, Copy)]
#[repr(u32)]
#[wasm_bindgen]
pub enum UtxoOrdering {
    Unordered,
    AscendingAmount,
    AscendingDaaScore,
}

#[derive(Default)]
pub struct Inner {
    entries: Mutex<Vec<UtxoEntryReference>>,
    ordered: AtomicU32,
}

impl Inner {
    fn new() -> Self {
        Self { entries: Mutex::new(vec![]), ordered: AtomicU32::new(UtxoOrdering::Unordered as u32) }
    }

    fn new_with_args(entries: Vec<UtxoEntryReference>, ordered: UtxoOrdering) -> Self {
        Self { entries: Mutex::new(entries), ordered: AtomicU32::new(ordered as u32) }
    }
}

/// a collection of UTXO entries
#[derive(Clone, Default)]
#[wasm_bindgen]
pub struct UtxoSet {
    inner: Arc<Inner>,
}

#[wasm_bindgen]
impl UtxoSet {
    pub fn insert(&mut self, utxo_entry: UtxoEntryReference) {
        self.inner.entries.lock().unwrap().push(utxo_entry);
        self.inner.ordered.store(UtxoOrdering::Unordered as u32, Ordering::SeqCst);
    }

    #[wasm_bindgen(js_name=select)]
    pub async fn select_utxos(&mut self, transaction_amount: u64, order: UtxoOrdering) -> Result<SelectionContext> {
        let data = self.select(transaction_amount, order).await?;
        Ok(data)
    }

    pub fn from(js_value: JsValue) -> Result<UtxoSet> {
        //log_info!("js_value: {:?}", js_value);
        let r: GetUtxosByAddressesResponse = from_value(js_value)?;
        //log_info!("r: {:?}", r);
        let entries = r.entries.into_iter().map(|entry| entry.into()).collect::<Vec<UtxoEntryReference>>();
        //log_info!("entries ...");
        let utxo_set = Self { inner: Arc::new(Inner::new_with_args(entries, UtxoOrdering::Unordered)) };
        //log_info!("utxo_set ...");
        Ok(utxo_set)
    }
}

impl UtxoSet {
    pub fn new() -> Self {
        Self { inner: Arc::new(Inner::new()) }
    }

    pub fn order(&self, order: UtxoOrdering) -> Result<()> {
        match order {
            UtxoOrdering::AscendingAmount => {
                self.inner.entries.lock().unwrap().sort_by_key(|a| a.as_ref().amount());
            }
            UtxoOrdering::AscendingDaaScore => {
                self.inner.entries.lock().unwrap().sort_by_key(|a| a.as_ref().block_daa_score());
            }
            UtxoOrdering::Unordered => {}
        }

        Ok(())
    }

    pub async fn chunks(&self, chunk_size: usize) -> Result<Vec<Vec<UtxoEntryReference>>> {
        let entries = self.inner.entries.lock().unwrap();
        let l = entries.chunks(chunk_size).map(|v| v.to_owned()).collect();
        Ok(l)
    }

    pub async fn select(&self, transaction_amount: u64, order: UtxoOrdering) -> Result<SelectionContext> {
        if self.inner.ordered.load(Ordering::SeqCst) != order as u32 {
            self.order(order)?;
        }

        let mut selected_entries = vec![];

        let total_selected_amount = self
            .inner
            .entries
            .lock()
            .unwrap()
            .iter()
            .scan(0u64, |total, entry| {
                if *total >= transaction_amount {
                    return None;
                }

                selected_entries.push(entry.clone());

                let amount = entry.as_ref().utxo_entry.amount;
                *total += amount;
                Some(amount)
            })
            .sum();

        Ok(SelectionContext { transaction_amount, total_selected_amount, selected_entries })
    }

    pub async fn calculate_balance(&self) -> Result<u64> {
        Ok(self.inner.entries.lock().unwrap().iter().map(|e| e.as_ref().utxo_entry.amount).sum())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[wasm_bindgen]
pub struct UtxoEntries(Arc<Vec<UtxoEntry>>);

#[wasm_bindgen]
impl UtxoEntries {
    #[wasm_bindgen(constructor)]
    pub fn js_ctor(js_value: JsValue) -> Result<UtxoEntries> {
        js_value.try_into()
    }
    #[wasm_bindgen(getter = items)]
    pub fn get_items_as_js_array(&self) -> JsValue {
        let items = self.0.as_ref().clone().into_iter().map(<UtxoEntry as Into<JsValue>>::into);
        Array::from_iter(items).into()
    }

    #[wasm_bindgen(setter = items)]
    pub fn set_items_from_js_array(&mut self, js_value: &JsValue) {
        let items = Array::from(js_value)
            .iter()
            .map(|js_value| ref_from_abi!(UtxoEntry, &js_value).unwrap_or_else(|err| panic!("invalid UtxoEntry: {err}")))
            .collect::<Vec<_>>();
        self.0 = Arc::new(items);
    }
}
impl UtxoEntries {
    pub fn items(&self) -> Arc<Vec<UtxoEntry>> {
        self.0.clone()
    }
}

impl From<UtxoEntries> for Vec<Option<UtxoEntry>> {
    fn from(value: UtxoEntries) -> Self {
        value.0.as_ref().iter().map(|entry| Some(entry.clone())).collect_vec()
    }
}

impl From<Vec<UtxoEntry>> for UtxoEntries {
    fn from(value: Vec<UtxoEntry>) -> Self {
        Self(Arc::new(value))
    }
}

impl From<UtxoEntries> for Vec<Option<cctx::UtxoEntry>> {
    fn from(value: UtxoEntries) -> Self {
        value.0.as_ref().iter().map(|entry| Some(entry.utxo_entry.clone())).collect_vec()
    }
}

impl TryFrom<Vec<Option<UtxoEntry>>> for UtxoEntries {
    type Error = Error;
    fn try_from(value: Vec<Option<UtxoEntry>>) -> std::result::Result<Self, Self::Error> {
        let mut list = vec![];
        for entry in value.into_iter() {
            list.push(entry.ok_or(Error::Custom("Unable to cast `Vec<Option<UtxoEntry>>` into `UtxoEntries`.".to_string()))?);
        }

        Ok(Self(Arc::new(list)))
    }
}

impl TryFrom<Vec<UtxoEntryReference>> for UtxoEntries {
    type Error = Error;
    fn try_from(value: Vec<UtxoEntryReference>) -> std::result::Result<Self, Self::Error> {
        let mut list = vec![];
        for entry in value.into_iter() {
            list.push(
                entry
                    .try_into()
                    .map_err(|_| Error::Custom("Unable to cast `Vec<UtxoEntryReference>` into `UtxoEntries`.".to_string()))?,
            );
        }

        Ok(Self(Arc::new(list)))
    }
}

impl TryFrom<JsValue> for UtxoEntries {
    type Error = Error;
    fn try_from(js_value: JsValue) -> std::result::Result<Self, Self::Error> {
        if !js_value.is_array() {
            return Err("UtxoEntries must be an array".into());
        }

        let mut list = vec![];
        for entry in Array::from(&js_value).iter() {
            list.push(match ref_from_abi!(UtxoEntry, &entry) {
                Ok(value) => value,
                Err(err) => {
                    if !entry.is_object() {
                        panic!("invalid UTXOEntry: {err}")
                    }
                    //log_trace!("entry: {:?}", entry);
                    let object = Object::from(entry);
                    let amount = object.get_u64("amount")?;
                    let script_public_key = ScriptPublicKey::try_from_jsvalue(
                        object.get("scriptPublicKey").map_err(|_| Error::Custom("missing `scriptPublicKey` property".into()))?,
                    )?;
                    let block_daa_score = object.get_u64("blockDaaScore")?;
                    let is_coinbase = object.get_bool("isCoinbase")?;
                    let address: Address = object.get_string("address")?.try_into()?;
                    let outpoint: TransactionOutpoint = object.get("outpoint")?.try_into()?;
                    UtxoEntry {
                        address: address.into(),
                        outpoint,
                        utxo_entry: cctx::UtxoEntry { amount, script_public_key, block_daa_score, is_coinbase },
                    }
                }
            })
        }
        Ok(Self(Arc::new(list)))
    }
}

use cctx::ScriptPublicKey;
use js_sys::Array;
use js_sys::Object;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[wasm_bindgen(js_name = "TxUtxoEntryList")]
pub struct UtxoEntryList(Arc<Vec<cctx::UtxoEntry>>);

#[wasm_bindgen]
impl UtxoEntryList {
    #[wasm_bindgen(constructor)]
    pub fn js_ctor(js_value: JsValue) -> std::result::Result<UtxoEntryList, JsError> {
        Ok(js_value.try_into()?)
    }
    #[wasm_bindgen(getter = items)]
    pub fn get_items_as_js_array(&self) -> JsValue {
        let items = self.0.as_ref().clone().into_iter().map(<cctx::UtxoEntry as Into<JsValue>>::into);
        Array::from_iter(items).into()
    }

    #[wasm_bindgen(setter = items)]
    pub fn set_items_from_js_array(&mut self, js_value: &JsValue) {
        let items = Array::from(js_value)
            .iter()
            .map(|js_value| ref_from_abi!(cctx::UtxoEntry, &js_value).unwrap_or_else(|err| panic!("invalid UTXOEntry: {err}")))
            .collect::<Vec<_>>();
        self.0 = Arc::new(items);
    }
}

impl From<UtxoEntryList> for Vec<Option<cctx::UtxoEntry>> {
    fn from(value: UtxoEntryList) -> Self {
        value.0.as_ref().iter().map(|entry| Some(entry.clone())).collect_vec()
    }
}

impl TryFrom<Vec<Option<cctx::UtxoEntry>>> for UtxoEntryList {
    type Error = Error;
    fn try_from(value: Vec<Option<cctx::UtxoEntry>>) -> Result<Self> {
        let mut list = vec![];
        for entry in value.into_iter() {
            list.push(entry.ok_or(Error::Custom("Unable to cast `Vec<Option<UtxoEntry>>` into `UtxoEntryList`.".to_string()))?);
        }

        Ok(Self(Arc::new(list)))
    }
}

impl TryFrom<JsValue> for UtxoEntryList {
    type Error = Error;
    fn try_from(js_value: JsValue) -> Result<Self> {
        if !js_value.is_array() {
            return Err("UtxoEntryList must be an array".into());
        }

        let mut list = vec![];
        for entry in Array::from(&js_value).iter() {
            list.push(match ref_from_abi!(cctx::UtxoEntry, &entry) {
                Ok(value) => value,
                Err(err) => {
                    if !entry.is_object() {
                        panic!("invalid UTXOEntry: {err}")
                    }
                    //log_trace!("entry: {:?}", entry);
                    let object = Object::from(entry);
                    let amount = object.get_u64("amount")?;
                    let script_public_key = ScriptPublicKey::try_from_jsvalue(
                        object.get("scriptPublicKey").map_err(|_| Error::Custom("missing `scriptPublicKey` property".into()))?,
                    )?;
                    let block_daa_score = object.get_u64("blockDaaScore")?;
                    let is_coinbase = object.get_bool("isCoinbase")?;
                    cctx::UtxoEntry { amount, script_public_key, block_daa_score, is_coinbase }
                }
            })
        }
        Ok(UtxoEntryList(Arc::new(list)))
    }
}