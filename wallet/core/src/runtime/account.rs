#[allow(unused_imports)]
use crate::accounts::{gen0::*, gen1::*, PubkeyDerivationManagerTrait, WalletDerivationManagerTrait};
use crate::address::{build_derivate_paths, AddressManager};
use crate::imports::*;
use crate::result::Result;
use crate::runtime::wallet::{Events, Wallet};
use crate::secret::Secret;
use crate::signer::sign_mutable_transaction;
use crate::storage::interface::AccessContext;
use crate::storage::{self, AccessContextT, PrvKeyData, PrvKeyDataId, PubKeyData};
use crate::tx::{LimitCalcStrategy, PaymentOutputs, VirtualTransaction};
use crate::utxo::{UtxoEntryId, UtxoEntryReference, UtxoSet};
use crate::AddressDerivationManager;
use faster_hex::hex_string;
use futures::future::join_all;
use kaspa_addresses::Prefix as AddressPrefix;
use kaspa_bip32::{ChildNumber, PrivateKey};
use kaspa_consensus_core::constants::SOMPI_PER_KASPA;
use kaspa_notify::listener::ListenerId;
use kaspa_notify::scope::{Scope, UtxosChangedScope};
use kaspa_rpc_core::api::notifications::Notification;
use serde::Serializer;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use workflow_core::abortable::Abortable;
use workflow_core::channel::{Channel, DuplexChannel};
use workflow_core::enums::u8_try_from;

#[derive(Default, Clone)]
pub struct Estimate {
    pub total_sompi: u64,
    pub fees_sompi: u64,
    pub utxos: usize,
    pub transactions: usize,
}

const DEFAULT_WINDOW_SIZE: u32 = 128;

#[derive(Default, Clone, Copy)]
pub enum ScanExtent {
    /// Scan until an empty range is found
    #[default]
    EmptyWindow,
    /// Scan until a specific depth (a particular derivation index)
    Depth(u32),
}

pub struct Scan {
    pub address_manager: Arc<AddressManager>,
    pub window_size: u32,
    pub extent: ScanExtent,
    pub balance: Arc<AtomicU64>,
}

impl Scan {
    pub fn new(address_manager: Arc<AddressManager>, balance: &Arc<AtomicU64>) -> Scan {
        Scan { address_manager, window_size: DEFAULT_WINDOW_SIZE, extent: ScanExtent::EmptyWindow, balance: balance.clone() }
    }
    pub fn new_with_args(
        address_manager: Arc<AddressManager>,
        window_size: u32,
        extent: ScanExtent,
        balance: &Arc<AtomicU64>,
    ) -> Scan {
        Scan { address_manager, window_size, extent, balance: balance.clone() }
    }
}

u8_try_from! {
    // #[derive(Describe, Debug, Default, Clone, Copy, Serialize, Deserialize, BorshSerialize, BorshDeserialize, Hash)]
    #[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, BorshSerialize, BorshDeserialize, Hash)]
    #[serde(rename_all = "lowercase")]
    #[wasm_bindgen]
    pub enum AccountKind {
        // #[describe("Legacy account (kaspanet.io Web Wallet, KDX)")]
        Legacy,
        #[default]
        // #[describe("Bip32 account")]
        Bip32,
        // #[describe("MultiSignature account")]
        MultiSig,
    }
}

impl ToString for AccountKind {
    fn to_string(&self) -> String {
        match self {
            AccountKind::Legacy => "legacy".to_string(),
            AccountKind::Bip32 => "bip32".to_string(),
            AccountKind::MultiSig => "multisig".to_string(),
        }
    }
}

impl FromStr for AccountKind {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "legacy" => Ok(AccountKind::Legacy),
            "bip32" => Ok(AccountKind::Bip32),
            "multisig" => Ok(AccountKind::MultiSig),
            _ => Err(Error::InvalidAccountKind),
        }
    }
}

// impl Display for AccountKind {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         write!(f, "{}", self)
//     }
// }

#[derive(Hash)]
struct AccountIdHashData {
    prv_key_data_id: PrvKeyDataId,
    ecdsa: bool,
    account_kind: AccountKind,
    account_index: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AccountId(pub(crate) u64);

impl AccountId {
    pub(crate) fn new(prv_key_data_id: &PrvKeyDataId, ecdsa: bool, account_kind: &AccountKind, account_index: u64) -> AccountId {
        let mut hasher = DefaultHasher::new();
        AccountIdHashData { prv_key_data_id: *prv_key_data_id, ecdsa, account_kind: *account_kind, account_index }.hash(&mut hasher);
        AccountId(hasher.finish())
    }
}

impl ToHex for AccountId {
    fn to_hex(&self) -> String {
        format!("{:x}", self.0)
    }
}

impl Serialize for AccountId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex_string(&self.0.to_be_bytes()))
    }
}

impl<'de> Deserialize<'de> for AccountId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let hex_str = <std::string::String as Deserialize>::deserialize(deserializer)?;
        let mut out = [0u8; 8];
        let mut input = [b'0'; 16];
        let start = input.len() - hex_str.len();
        input[start..].copy_from_slice(hex_str.as_bytes());
        faster_hex::hex_decode(&input, &mut out).map_err(serde::de::Error::custom)?;
        Ok(AccountId(u64::from_be_bytes(out)))
    }
}

impl std::fmt::Display for AccountId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex_string(&self.0.to_be_bytes()))
    }
}

pub struct Inner {
    pub listener_id: Option<ListenerId>,
    pub stored: storage::Account,
}

/// Wallet `Account` data structure. An account is typically a single
/// HD-key derivation (derived from a wallet or from an an external secret)
// #[wasm_bindgen(inspectable)]
pub struct Account {
    // #[wasm_bindgen(skip)]
    pub id: AccountId,
    inner: Arc<Mutex<Inner>>,
    wallet: Arc<Wallet>,
    utxos: UtxoSet,
    // balance: Arc<AtomicU64>,
    balance: Mutex<Option<u64>>,
    is_connected: AtomicBool,
    // #[wasm_bindgen(js_name = "accountKind")]
    pub account_kind: AccountKind,
    pub account_index: u64,
    // #[wasm_bindgen(skip)]
    pub prv_key_data_id: PrvKeyDataId,
    pub ecdsa: bool,
    // #[wasm_bindgen(skip)]
    pub derivation: Arc<AddressDerivationManager>,
    // #[wasm_bindgen(skip)]
    pub task_ctl: DuplexChannel,
    // #[wasm_bindgen(skip)]
    pub notification_channel: Channel<Notification>,
}

impl Account {
    pub async fn try_new_arc_with_args(
        wallet: &Arc<Wallet>,
        name: &str,
        title: &str,
        account_kind: AccountKind,
        account_index: u64,
        prv_key_data_id: PrvKeyDataId,
        pub_key_data: PubKeyData,
        ecdsa: bool,
        address_prefix: AddressPrefix,
    ) -> Result<Arc<Self>> {
        let minimum_signatures = pub_key_data.minimum_signatures.unwrap_or(1) as usize;
        let derivation =
            AddressDerivationManager::new(address_prefix, account_kind, &pub_key_data, ecdsa, minimum_signatures, None, None).await?;

        let stored = storage::Account::new(
            name.to_string(),
            title.to_string(),
            account_kind,
            account_index,
            false,
            pub_key_data.clone(),
            prv_key_data_id,
            ecdsa,
            pub_key_data.minimum_signatures.unwrap_or(1),
            pub_key_data.cosigner_index.unwrap_or(0),
        );

        let inner = Inner { listener_id: None, stored };

        Ok(Arc::new(Account {
            id: AccountId::new(&prv_key_data_id, ecdsa, &account_kind, account_index),
            wallet: wallet.clone(),
            utxos: UtxoSet::default(),
            balance: Mutex::new(None), // Arc::new(AtomicU64::new(0)),
            is_connected: AtomicBool::new(false),
            inner: Arc::new(Mutex::new(inner)),
            account_kind,
            account_index,
            prv_key_data_id,
            ecdsa: false,
            derivation,
            task_ctl: DuplexChannel::oneshot(),
            notification_channel: Channel::<Notification>::unbounded(),
        }))
    }

    pub async fn try_new_arc_from_storage(
        wallet: &Arc<Wallet>,
        stored: &storage::Account,
        address_prefix: AddressPrefix,
    ) -> Result<Arc<Self>> {
        let minimum_signatures = stored.pub_key_data.minimum_signatures.unwrap_or(1) as usize;
        let derivation = AddressDerivationManager::new(
            address_prefix,
            stored.account_kind,
            &stored.pub_key_data,
            stored.ecdsa,
            minimum_signatures,
            None,
            None,
        )
        .await?;

        let inner = Inner { listener_id: None, stored: stored.clone() };

        Ok(Arc::new(Account {
            id: AccountId::new(&stored.prv_key_data_id, stored.ecdsa, &stored.account_kind, stored.account_index),
            wallet: wallet.clone(),
            utxos: UtxoSet::default(),
            balance: Mutex::new(None), //Arc::new(AtomicU64::new(0)),
            is_connected: AtomicBool::new(false),
            inner: Arc::new(Mutex::new(inner)),
            account_kind: stored.account_kind,
            account_index: stored.account_index,
            prv_key_data_id: stored.prv_key_data_id,
            ecdsa: stored.ecdsa,
            derivation,
            task_ctl: DuplexChannel::oneshot(),
            notification_channel: Channel::<Notification>::unbounded(),
        }))
    }

    pub async fn update_balance(self: &Arc<Account>) -> Result<u64> {
        let balance = self.utxos.calculate_balance().await?;
        self.balance.lock().unwrap().replace(balance);
        self.wallet
            .multiplexer
            .broadcast(Events::BalanceUpdate { balance, account_id: self.id })
            .await
            .map_err(|_| Error::Custom("multiplexer channel error during update_balance".to_string()))?;
        Ok(balance)
    }

    pub fn is_connected(&self) -> bool {
        self.is_connected.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn name(&self) -> String {
        self.inner.lock().unwrap().stored.name.clone()
    }

    pub fn balance(&self) -> Option<u64> {
        *self.balance.lock().unwrap()
    }

    pub fn balance_as_string(&self) -> Option<String> {
        self.balance().map(|b| {
            let f = b / SOMPI_PER_KASPA;
            format!("{}", f)
        })
    }

    pub fn get_ls_string(&self) -> String {
        let name = self.name();
        let balance = self.balance_as_string().map(|s| format!("{s} KAS")).unwrap_or_else(|| "n/a".to_string());
        format!("{name} - {balance}")
    }

    pub fn inner(&self) -> MutexGuard<Inner> {
        self.inner.lock().unwrap()
    }

    pub async fn scan_address_manager(self: &Arc<Self>, scan: Scan) -> Result<()> {
        let mut cursor = 0;

        // let mut balance = 0;
        let mut last_address_index = std::cmp::max(scan.address_manager.index()?, scan.window_size);

        'scan: loop {
            let first = cursor;
            let last = if cursor == 0 { last_address_index } else { cursor + scan.window_size };
            // window_size = scan.window_size;
            cursor = last;

            log_info!("first: {}, last: {}\r\n", first, last);

            let addresses = scan.address_manager.get_range(first..last).await?;

            self.subscribe_utxos_changed(&addresses).await?;
            let resp = self.wallet.rpc().get_utxos_by_addresses(addresses).await?;
            let refs: Vec<UtxoEntryReference> = resp.into_iter().map(UtxoEntryReference::from).collect();
            //println!("{}", format!("addresses:{:#?}", address_str).replace('\n', "\r\n"));
            //println!("{}", format!("resp:{:#?}", resp.get(0).and_then(|a|a.address.clone())).replace('\n', "\r\n"));

            for utxo_ref in refs.iter() {
                if let Some(address) = utxo_ref.utxo.address.as_ref() {
                    if let Some(utxo_address_index) = scan.address_manager.inner().address_to_index_map.get(address) {
                        if last_address_index < *utxo_address_index {
                            last_address_index = *utxo_address_index;
                        }
                    } else {
                        panic!("Account::scan_address_manager() has received an unknown address: `{address}`");
                    }
                }
            }

            self.utxos.extend(&refs);
            let balance = refs.iter().map(|r| r.as_ref().amount()).sum::<u64>();

            if balance != 0 {
                println!("scan_address_managet() balance increment: {balance}");
                scan.balance.fetch_add(balance, Ordering::SeqCst);
                // balance += utxo_balance;

                // - TODO - post balance update to the multiplexer?
            } else {
                match &scan.extent {
                    ScanExtent::EmptyWindow => {
                        if cursor > last_address_index + scan.window_size {
                            break 'scan;
                        }
                    }
                    ScanExtent::Depth(depth) => {
                        if &cursor > depth {
                            break 'scan;
                        }
                    }
                }
            }
        }

        scan.address_manager.set_index(last_address_index)?;

        Ok(())
    }

    pub async fn scan_utxos(self: &Arc<Self>, window_size: Option<u32>, extent: Option<u32>) -> Result<()> {
        self.utxos.clear();
        // self.balance.store(0, Ordering::SeqCst);

        let balance = Arc::new(AtomicU64::new(0));

        let window_size = window_size.unwrap_or(DEFAULT_WINDOW_SIZE);
        let extent = match extent {
            Some(depth) => ScanExtent::Depth(depth),
            None => ScanExtent::EmptyWindow,
        };

        let scans = vec![
            self.scan_address_manager(Scan::new_with_args(self.derivation.receive_address_manager(), window_size, extent, &balance)),
            self.scan_address_manager(Scan::new_with_args(self.derivation.change_address_manager(), window_size, extent, &balance)),
        ];

        join_all(scans).await.into_iter().collect::<Result<Vec<_>>>()?;

        // let balance = balance.load(std::sync::atomic::Ordering::SeqCst);
        self.balance.lock().unwrap().replace(balance.load(std::sync::atomic::Ordering::SeqCst));
        // - TODO - post balance updates to the wallet

        Ok(())
    }

    // - TODO
    pub async fn scan_block(self: &Arc<Self>, addresses: Vec<Address>) -> Result<Vec<UtxoEntryReference>> {
        self.subscribe_utxos_changed(&addresses).await?;
        let resp = self.wallet.rpc().get_utxos_by_addresses(addresses).await?;
        let refs: Vec<UtxoEntryReference> = resp.into_iter().map(UtxoEntryReference::from).collect();
        Ok(refs)
    }

    pub async fn estimate(&self, _address: &Address, _amount_sompi: u64, _priority_fee_sompi: u64) -> Result<Estimate> {
        todo!()
        // Ok(())
    }

    pub async fn send(
        &self,
        outputs: &PaymentOutputs,
        priority_fee_sompi: Option<u64>,
        _include_fees_in_amount: bool,
        wallet_secret: Secret,
        payment_secret: Option<Secret>,
        abortable: &Abortable,
    ) -> Result<Vec<kaspa_hashes::Hash>> {
        let mut ctx = self.utxos.create_selection_context();
        // let transaction_amount = outputs.amount() + priority_fee_sompi.as_ref().cloned().unwrap_or_default();
        // ctx.select(transaction_amount);
        // let utxo_selection = self.utxos.select_utxos(transaction_amount, UtxoOrdering::AscendingAmount, true).await?;

        let change_address = self.change_address().await?;
        let payload = vec![];
        let sig_op_count = self.inner().stored.pub_key_data.keys.len() as u8;
        let minimum_signatures = self.inner().stored.minimum_signatures;
        let vt = VirtualTransaction::new(
            sig_op_count,
            minimum_signatures,
            &mut ctx,
            outputs,
            &change_address,
            priority_fee_sompi,
            payload,
            LimitCalcStrategy::inputs(80),
            abortable,
        )
        .await?;

        let addresses = ctx.addresses();
        let indexes = self.derivation.addresses_indexes(&addresses)?;
        let receive_indexes = indexes.0;
        let change_indexes = indexes.1;

        let access_ctx: Arc<dyn AccessContextT> = Arc::new(AccessContext::new(wallet_secret));
        let keydata = self
            .wallet
            .store()
            .as_prv_key_data_store()?
            .load_key_data(&access_ctx, &self.prv_key_data_id)
            .await?
            .ok_or(Error::PrivateKeyNotFound(self.prv_key_data_id.to_hex()))?;

        let private_keys = self.create_private_keys(keydata, payment_secret, receive_indexes, change_indexes)?;
        let private_keys = &private_keys.iter().map(|k| k.to_bytes()).collect::<Vec<_>>();
        let mut tx_ids = vec![];
        for mtx in vt.transactions().clone() {
            let mtx = sign_mutable_transaction(mtx, private_keys, true)?;
            let id = self.wallet.rpc().submit_transaction(mtx.try_into()?, false).await?;
            //println!("id: {id}\r\n");
            tx_ids.push(id);
        }

        ctx.commit()?;

        Ok(tx_ids)
    }

    fn create_private_keys(
        &self,
        keydata: PrvKeyData,
        payment_secret: Option<Secret>,
        receive_indexes: Vec<u32>,
        change_indexes: Vec<u32>,
    ) -> Result<Vec<secp256k1::SecretKey>> {
        let payload = keydata.payload.decrypt(payment_secret.as_ref())?;
        let xkey = payload.private_key(payment_secret.as_ref())?;

        let cosigner_index = self.inner().stored.pub_key_data.cosigner_index.unwrap_or(0);
        let paths = build_derivate_paths(self.account_kind, self.account_index, cosigner_index)?;
        let receive_xkey = xkey.clone().derive_path(paths.0)?;
        let change_xkey = xkey.derive_path(paths.1)?;

        let mut private_keys = vec![];
        for index in receive_indexes {
            private_keys.push(*receive_xkey.derive_child(ChildNumber::new(index, false)?)?.private_key());
        }
        for index in change_indexes {
            private_keys.push(*change_xkey.derive_child(ChildNumber::new(index, false)?)?.private_key());
        }

        Ok(private_keys)
    }

    pub async fn address(&self) -> Result<Address> {
        self.receive_address_manager()?.current_address().await
    }

    pub async fn change_address(&self) -> Result<Address> {
        self.change_address_manager()?.current_address().await
    }

    pub fn receive_address_manager(&self) -> Result<Arc<AddressManager>> {
        Ok(self.derivation.receive_address_manager())
    }

    pub fn change_address_manager(&self) -> Result<Arc<AddressManager>> {
        Ok(self.derivation.change_address_manager())
    }

    pub async fn new_receive_address(self: &Arc<Self>) -> Result<String> {
        let address = self.receive_address_manager()?.new_address().await?;
        self.subscribe_utxos_changed(&[address.clone()]).await?;
        Ok(address.into())
    }

    pub async fn new_change_address(self: &Arc<Self>) -> Result<String> {
        let address = self.change_address_manager()?.new_address().await?;
        self.subscribe_utxos_changed(&[address.clone()]).await?;
        Ok(address.into())
    }

    pub async fn sign(&self) -> Result<()> {
        Ok(())
    }

    pub async fn sweep(&self) -> Result<()> {
        Ok(())
    }

    pub async fn create_unsigned_transaction(&self) -> Result<()> {
        Ok(())
    }

    // -

    /// Start Account service task
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        // self.start_task().await
        if self.wallet.is_connected() {
            self.connect().await?;
        }

        Ok(())
    }

    /// Stop Account service task
    pub async fn stop(&self) -> Result<()> {
        // self.stop_task().await
        Ok(())
    }

    /// handle connection event
    pub async fn connect(self: &Arc<Self>) -> Result<()> {
        self.wallet.active_accounts().insert(self.clone());

        // self.is_connected.store(true, Ordering::SeqCst);
        // self.register_notification_listener().await?;
        self.scan_utxos(Some(128), None).await?;
        Ok(())
    }

    /// handle disconnection event
    pub async fn disconnect(&self) -> Result<()> {
        self.wallet.active_accounts().remove(&self.id);
        // self.is_connected.store(false, Ordering::SeqCst);
        // self.unregister_notification_listener().await?;
        Ok(())
    }

    fn listener_id(&self) -> Option<ListenerId> {
        self.inner.lock().unwrap().listener_id
    }

    async fn subscribe_utxos_changed(self: &Arc<Self>, addresses: &[Address]) -> Result<()> {
        self.wallet.address_to_account_map().lock().unwrap().extend(addresses.iter().map(|a| (a.clone(), self.clone())));
        let listener_id = self.wallet.listener_id();
        let utxos_changed_scope = UtxosChangedScope { addresses: addresses.to_vec() };
        self.wallet.rpc.start_notify(listener_id, Scope::UtxosChanged(utxos_changed_scope)).await?;

        Ok(())
    }

    #[allow(dead_code)]
    async fn unsubscribe_utxos_changed(self: &Arc<Self>, addresses: &[Address]) -> Result<()> {
        self.wallet.address_to_account_map().lock().unwrap().extend(addresses.iter().map(|a| (a.clone(), self.clone())));

        let listener_id = self
            .listener_id()
            .expect("subscribe_utxos_changed() requires `listener_id` (must call `register_notification_listener()` before use)");
        let utxos_changed_scope = UtxosChangedScope { addresses: addresses.to_vec() };
        self.wallet.rpc.stop_notify(listener_id, Scope::UtxosChanged(utxos_changed_scope)).await?;

        Ok(())
    }

    pub(crate) async fn handle_utxo_added(&self, utxos: Vec<UtxoEntryReference>) -> Result<()> {
        self.utxos.insert(utxos);
        Ok(())
    }

    pub(crate) async fn handle_utxo_removed(&self, utxo_ids: Vec<UtxoEntryId>) -> Result<bool> {
        Ok(self.utxos.remove(utxo_ids))
    }
}

// #[wasm_bindgen]
// impl Account {
//     #[wasm_bindgen(getter)]
//     pub fn get_balance(&self) -> JsValue {
//         // self.balance.load(std::sync::atomic::Ordering::SeqCst)
//     }
// }

// impl Display for Account {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         write!(f, "{} ({})", self.name.as_ref().unwrap_or(&"-".to_string()), self.id.to_hex())
//     }
// }

#[derive(Default, Clone)]
pub struct AccountMap(Arc<Mutex<HashMap<AccountId, Arc<Account>>>>);

impl AccountMap {
    pub fn inner(&self) -> MutexGuard<HashMap<AccountId, Arc<Account>>> {
        self.0.lock().unwrap()
    }

    pub fn clear(&self) {
        self.inner().clear();
    }

    pub fn get(&self, account_id: &AccountId) -> Option<Arc<Account>> {
        self.inner().get(account_id).cloned()
    }

    pub fn extend(&self, accounts: Vec<Arc<Account>>) {
        let mut map = self.inner();
        let accounts = accounts.into_iter().map(|a| (a.id, a)); //.collect::<Vec<_>>();
        map.extend(accounts);
    }

    pub fn insert(&self, account: Arc<Account>) {
        self.inner().insert(account.id, account);
    }

    pub fn remove(&self, id: &AccountId) {
        self.inner().remove(id);
    }

    pub fn cloned_flat_list(&self) -> Vec<Arc<Account>> {
        self.inner().values().cloned().collect()
    }
}
