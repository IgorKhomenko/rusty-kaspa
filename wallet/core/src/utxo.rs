use crate::imports::*;
use crate::result::Result;
use crate::tx::{TransactionOutpoint, TransactionOutpointInner};
use itertools::Itertools;
use kaspa_rpc_core::{GetUtxosByAddressesResponse, RpcUtxosByAddressesEntry};
use serde_wasm_bindgen::from_value;
use sorted_insert::SortedInsertBinary;
use std::cmp::Ordering;
use std::collections::HashMap;
use workflow_core::time::{Duration, Instant};
use workflow_wasm::abi::{ref_from_abi, TryFromJsValue};

pub type UtxoEntryId = TransactionOutpointInner;

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

#[derive(Clone, Debug, Serialize, Deserialize, TryFromJsValue)]
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

    #[wasm_bindgen(js_name = "getId")]
    pub fn id_string(&self) -> String {
        self.utxo.outpoint.id_string()
    }

    pub fn amount(&self) -> u64 {
        self.utxo.amount()
    }
}

impl UtxoEntryReference {
    pub fn id(&self) -> UtxoEntryId {
        self.utxo.outpoint.inner().clone()
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

impl From<UtxoEntry> for UtxoEntryReference {
    fn from(entry: UtxoEntry) -> Self {
        Self { utxo: Arc::new(entry) }
    }
}

impl Eq for UtxoEntryReference {}

impl PartialEq for UtxoEntryReference {
    fn eq(&self, other: &Self) -> bool {
        self.amount() == other.amount()
    }
}

impl PartialOrd for UtxoEntryReference {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.amount().cmp(&other.amount()))
    }
}

impl Ord for UtxoEntryReference {
    fn cmp(&self, other: &Self) -> Ordering {
        self.amount().cmp(&other.amount())
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
#[derive(Default, Clone, Copy)]
#[repr(u32)]
#[wasm_bindgen]
pub enum UtxoOrdering {
    #[default]
    Unordered,
    AscendingAmount,
    AscendingDaaScore,
}

pub struct Consumed {
    entry: UtxoEntryReference,
    instant: Instant,
}

impl From<(UtxoEntryReference, &Instant)> for Consumed {
    fn from((entry, instant): (UtxoEntryReference, &Instant)) -> Self {
        Self { entry, instant: *instant }
    }
}

#[derive(Default)]
pub struct Inner {
    entries: Vec<UtxoEntryReference>,
    consumed: HashMap<UtxoEntryId, Consumed>,
    map: HashMap<UtxoEntryId, UtxoEntryReference>,
}

impl Inner {
    fn new() -> Self {
        Self { entries: vec![], map: HashMap::default(), consumed: HashMap::default() }
    }

    fn new_with_args(entries: Vec<UtxoEntryReference>) -> Self {
        Self { entries, map: HashMap::default(), consumed: HashMap::default() }
    }
}

pub struct UtxoSetIterator {
    utxos: UtxoSet,
    cursor: usize,
}

impl UtxoSetIterator {
    pub fn new(utxos: UtxoSet) -> Self {
        Self { utxos, cursor: 0 }
    }
}

impl Stream for UtxoSetIterator {
    type Item = UtxoEntryReference;
    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let entry = self.utxos.inner.lock().unwrap().entries.get(self.cursor).cloned();
        self.cursor += 1;
        Poll::Ready(entry)
    }
}

#[wasm_bindgen]
pub struct UtxoSelectionContext {
    utxos: UtxoSet,
    stream: Pin<Box<dyn Stream<Item = UtxoEntryReference> + Send>>,
    selected_entries: Vec<UtxoEntryReference>,
    selected_amount: u64,
}

impl UtxoSelectionContext {
    pub fn new(utxos: UtxoSet) -> Self {
        Self {
            utxos: utxos.clone(),
            stream: Box::pin(UtxoSetIterator { utxos, cursor: 0 }),
            selected_entries: Vec::default(),
            selected_amount: 0,
        }
    }

    pub fn addresses(&self) -> Vec<Address> {
        self.selected_entries.iter().map(|u| u.utxo.address.clone().unwrap()).collect::<Vec<Address>>()
    }

    pub fn selected_amount(&self) -> u64 {
        self.selected_amount
    }

    pub fn selected_entries(&self) -> &Vec<UtxoEntryReference> {
        &self.selected_entries
    }

    pub async fn select(&mut self, selection_amount: u64) -> Result<Vec<UtxoEntryReference>> {
        let mut amount = 0u64;
        let mut vec = vec![];
        while let Some(entry) = self.stream.next().await {
            amount += entry.amount();
            self.selected_entries.push(entry.clone());
            vec.push(entry);

            if amount >= selection_amount {
                break;
            }
        }

        if amount < selection_amount {
            Err(Error::InsufficientFunds)
        } else {
            self.selected_amount = amount;
            Ok(vec)
        }
    }

    pub fn commit(self) -> Result<()> {
        let mut inner = self.utxos.inner();
        inner.entries.retain(|entry| self.selected_entries.contains(entry));
        let now = Instant::now();
        self.selected_entries.into_iter().for_each(|entry| {
            inner.consumed.insert(entry.id(), (entry, &now).into());
        });

        Ok(())
    }
}

/// a collection of UTXO entries
#[derive(Clone, Default)]
#[wasm_bindgen]
pub struct UtxoSet {
    inner: Arc<Mutex<Inner>>,
}

#[wasm_bindgen]
impl UtxoSet {
    pub fn clear(&self) {
        let mut inner = self.inner();
        inner.map.clear();
        inner.entries.clear();
        inner.consumed.clear()
    }

    #[wasm_bindgen(js_name = "remove")]
    pub fn remove_js(&self, id_string: String) -> bool {
        let mut inner = self.inner();
        let index = match inner.entries.iter().position(|entry| entry.id_string() == id_string) {
            Some(index) => index,
            None => return false,
        };

        let entry = inner.entries.remove(index);
        inner.map.remove(&entry.id());

        true
    }

    // pub fn exists(&self, utxo_entry: &UtxoEntryReference) -> bool {
    //     let id = utxo_entry.id();
    //     self.inner.entries.lock().unwrap().iter().find(|entry| entry.id() == id).cloned().is_some()
    // }

    // pub fn find(&self, id: String) -> Option<UtxoEntryReference> {
    //     self.inner.entries.lock().unwrap().iter().find(|entry| entry.utxo.outpoint.id() == id).cloned()
    // }

    // #[wasm_bindgen(js_name=select)]
    // pub async fn select_utxos(&self, transaction_amount: u64, order: UtxoOrdering, mark_utxo: bool) -> Result<SelectionContext> {
    //     let data = self.select(transaction_amount, order, mark_utxo).await?;
    //     Ok(data)
    // }

    pub fn from(js_value: JsValue) -> Result<UtxoSet> {
        //log_info!("js_value: {:?}", js_value);
        let r: GetUtxosByAddressesResponse = from_value(js_value)?;
        //log_info!("r: {:?}", r);
        let mut entries = r.entries.into_iter().map(|entry| entry.into()).collect::<Vec<UtxoEntryReference>>();
        //log_info!("entries ...");
        entries.sort();

        let utxo_set = Self { inner: Arc::new(Mutex::new(Inner::new_with_args(entries))) };
        //log_info!("utxo_set ...");
        Ok(utxo_set)
    }
}

impl UtxoSet {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(Inner::new())) }
    }

    pub fn inner(&self) -> MutexGuard<Inner> {
        self.inner.lock().unwrap()
    }

    pub fn create_selection_context(&self) -> UtxoSelectionContext {
        UtxoSelectionContext::new(self.clone())
    }

    /// Insert `utxo_entry` into the `UtxoSet`.
    /// NOTE: The insert will be ignored if already present in the inner map.
    pub fn insert(&self, utxo_entries: Vec<UtxoEntryReference>) {
        let mut inner = self.inner();

        for utxo_entry in utxo_entries.into_iter() {
            if let std::collections::hash_map::Entry::Vacant(e) = inner.map.entry(utxo_entry.id()) {
                e.insert(utxo_entry.clone());
                inner.entries.sorted_insert_asc_binary(utxo_entry);
            } else {
                log_warning!("ignoring duplicate utxo entry insert");
            }
        }
    }

    pub fn remove(&self, id: Vec<UtxoEntryId>) -> bool {
        let mut inner = self.inner();

        let mut removed = vec![];
        for id in id.iter() {
            if inner.map.remove(id).is_some() {
                removed.push(id);
            }
        }

        for id in removed.into_iter() {
            if inner.consumed.remove(id).is_none() {
                inner.entries.retain(|entry| &entry.id() != id);
            }
        }

        true
    }

    pub fn extend(&self, utxo_entries: &[UtxoEntryReference]) {
        let mut inner = self.inner();
        for entry in utxo_entries {
            if inner.map.insert(entry.id(), entry.clone()).is_none() {
                inner.entries.sorted_insert_asc_binary(entry.clone());
            }
        }
        // self.ordered.store(UtxoOrdering::Unordered as u32, Ordering::SeqCst);
    }

    pub async fn chunks(&self, chunk_size: usize) -> Result<Vec<Vec<UtxoEntryReference>>> {
        let entries = &self.inner().entries;
        let l = entries.chunks(chunk_size).map(|v| v.to_owned()).collect();
        Ok(l)
    }

    pub async fn recover_consumed_utxos(&self) -> Result<()> {
        let checkpoint = Instant::now().checked_sub(Duration::from_secs(60)).unwrap();

        let mut inner = self.inner();

        let mut removed = vec![];
        inner.consumed.retain(|_, consumed| {
            if consumed.instant < checkpoint {
                // if timedout return back to entries
                removed.push(consumed.entry.clone());
                false
            } else {
                true
            }
        });

        removed.into_iter().for_each(|entry| {
            inner.entries.sorted_insert_asc_binary(entry);
        });

        Ok(())
    }

    /*

    // pub async fn select(&self, transaction_amount: u64, order: UtxoOrdering, mark_utxo: bool) -> Result<SelectionContext> {
    pub async fn select(&self, transaction_amount: u64, mark_utxo: bool) -> Result<SelectionContext> {
        // if self.ordered.load(Ordering::SeqCst) != order as u32 {
        //     self.order(order)?;
        // }

        // TODO: move to ticker callback
        self.update_inuse_utxos().await?;

        const FEE_PER_INPUT: u64 = 1124;

        let mut selected_entries = vec![];
        let mut in_use = vec![];
        let total_selected_amount = {
            let inner = self.inner();
            inner
                .entries
                .iter()
                .scan(0u64, |total, entry| {
                    let outpoint = entry.as_ref().outpoint.inner().clone();
                    if inner.in_use.contains_key(&outpoint) {
                        return Some(0);
                    }

                    if mark_utxo {
                        in_use.push(outpoint);
                    }
                    if *total >= transaction_amount + selected_entries.len() as u64 * FEE_PER_INPUT {
                        return None;
                    }

                    selected_entries.push(entry.clone());

                    let amount = entry.as_ref().utxo_entry.amount;
                    *total += amount;
                    Some(amount)
                })
                .sum()
        };

        if mark_utxo {
            let map = &mut self.inner().in_use;
            let now = Instant::now();
            for outpoint in in_use {
                map.insert(outpoint, now);
            }
        }

        Ok(SelectionContext { transaction_amount, total_selected_amount, selected_entries })
    }

    */

    pub async fn calculate_balance(&self) -> Result<u64> {
        Ok(self.inner().entries.iter().map(|e| e.as_ref().utxo_entry.amount).sum())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[wasm_bindgen]
pub struct UtxoEntries(Arc<Vec<UtxoEntryReference>>);

#[wasm_bindgen]
impl UtxoEntries {
    #[wasm_bindgen(constructor)]
    pub fn js_ctor(js_value: JsValue) -> Result<UtxoEntries> {
        js_value.try_into()
    }
    #[wasm_bindgen(getter = items)]
    pub fn get_items_as_js_array(&self) -> JsValue {
        let items = self.0.as_ref().clone().into_iter().map(<UtxoEntryReference as Into<JsValue>>::into);
        Array::from_iter(items).into()
    }

    #[wasm_bindgen(setter = items)]
    pub fn set_items_from_js_array(&mut self, js_value: &JsValue) {
        let items = Array::from(js_value)
            .iter()
            .map(|js_value| {
                ref_from_abi!(UtxoEntryReference, &js_value).unwrap_or_else(|err| panic!("invalid UtxoEntryReference: {err}"))
            })
            .collect::<Vec<_>>();
        self.0 = Arc::new(items);
    }
}
impl UtxoEntries {
    pub fn items(&self) -> Arc<Vec<UtxoEntryReference>> {
        self.0.clone()
    }
}

impl From<UtxoEntries> for Vec<Option<UtxoEntry>> {
    fn from(value: UtxoEntries) -> Self {
        value.0.as_ref().iter().map(|entry| Some(entry.as_ref().clone())).collect_vec()
    }
}

impl From<Vec<UtxoEntry>> for UtxoEntries {
    fn from(items: Vec<UtxoEntry>) -> Self {
        Self(Arc::new(items.into_iter().map(UtxoEntryReference::from).collect::<_>()))
    }
}

impl From<UtxoEntries> for Vec<Option<cctx::UtxoEntry>> {
    fn from(value: UtxoEntries) -> Self {
        value.0.as_ref().iter().map(|entry| Some(entry.utxo.utxo_entry.clone())).collect_vec()
    }
}

impl TryFrom<Vec<Option<UtxoEntry>>> for UtxoEntries {
    type Error = Error;
    fn try_from(value: Vec<Option<UtxoEntry>>) -> std::result::Result<Self, Self::Error> {
        let mut list = vec![];
        for entry in value.into_iter() {
            list.push(entry.ok_or(Error::Custom("Unable to cast `Vec<Option<UtxoEntry>>` into `UtxoEntries`.".to_string()))?.into());
        }

        Ok(Self(Arc::new(list)))
    }
}

impl TryFrom<Vec<UtxoEntryReference>> for UtxoEntries {
    type Error = Error;
    fn try_from(list: Vec<UtxoEntryReference>) -> std::result::Result<Self, Self::Error> {
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
            list.push(match ref_from_abi!(UtxoEntryReference, &entry) {
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
                    .into()
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
