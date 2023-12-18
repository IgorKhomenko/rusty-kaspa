use crate::account::Inner;
use crate::derivation::{AddressDerivationManager, AddressDerivationManagerTrait};
use crate::imports::*;

pub const BIP32_ACCOUNT_MAGIC: u32 = 0x42503332;
pub const BIP32_ACCOUNT_VERSION: u32 = 0;
pub const BIP32_ACCOUNT_KIND: &str = "kaspa-bip32-standard";

pub struct Ctor {}

#[async_trait]
impl Factory for Ctor {
    fn name(&self) -> String {
        "bip32/bip44".to_string()
    }

    fn description(&self) -> String {
        "Kaspa Core HD Wallet Account".to_string()
    }

    async fn try_load(
        &self,
        wallet: &Arc<Wallet>,
        storage: &AccountStorage,
        meta: Option<Arc<AccountMetadata>>,
    ) -> Result<Arc<dyn Account>> {
        Ok(Arc::new(bip32::Bip32::try_load(wallet, storage, meta).await?))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub struct Storable {
    pub xpub_keys: Arc<Vec<ExtendedPublicKeySecp256k1>>,
    pub account_index: u64,
    pub ecdsa: bool,
}

impl Storable {
    pub fn new(account_index: u64, xpub_keys: Arc<Vec<ExtendedPublicKeySecp256k1>>, ecdsa: bool) -> Self {
        Self { account_index, xpub_keys, ecdsa }
    }

    pub fn try_load(storage: &AccountStorage) -> Result<Self> {
        Ok(Self::try_from_slice(storage.serialized.as_slice())?)
    }
}

impl BorshSerialize for Storable {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        StorageHeader::new(BIP32_ACCOUNT_MAGIC, BIP32_ACCOUNT_VERSION).serialize(writer)?;
        BorshSerialize::serialize(&self.xpub_keys, writer)?;
        BorshSerialize::serialize(&self.account_index, writer)?;
        BorshSerialize::serialize(&self.ecdsa, writer)?;

        Ok(())
    }
}

impl BorshDeserialize for Storable {
    fn deserialize(buf: &mut &[u8]) -> IoResult<Self> {
        let StorageHeader { version: _, .. } =
            StorageHeader::deserialize(buf)?.try_magic(BIP32_ACCOUNT_MAGIC)?.try_version(BIP32_ACCOUNT_VERSION)?;

        let xpub_keys = BorshDeserialize::deserialize(buf)?;
        let account_index = BorshDeserialize::deserialize(buf)?;
        let ecdsa = BorshDeserialize::deserialize(buf)?;

        Ok(Self { xpub_keys, account_index, ecdsa })
    }
}

pub struct Bip32 {
    inner: Arc<Inner>,
    prv_key_data_id: PrvKeyDataId,
    account_index: u64,
    xpub_keys: ExtendedPublicKeys,
    ecdsa: bool,
    derivation: Arc<AddressDerivationManager>,
}

impl Bip32 {
    pub async fn try_new(
        wallet: &Arc<Wallet>,
        name: Option<String>,
        prv_key_data_id: PrvKeyDataId,
        account_index: u64,
        xpub_keys: ExtendedPublicKeys,
        ecdsa: bool,
    ) -> Result<Self> {
        let storable = Storable::new(account_index, xpub_keys.clone(), ecdsa);
        let settings = AccountSettings { name, ..Default::default() };
        let (id, storage_key) = make_account_hashes(from_bip32(&prv_key_data_id, &storable));
        let inner = Arc::new(Inner::new(wallet, id, storage_key, settings));

        let derivation =
            AddressDerivationManager::new(wallet, BIP32_ACCOUNT_KIND.into(), &xpub_keys, ecdsa, 0, None, 1, Default::default())
                .await?;

        Ok(Self { inner, prv_key_data_id, account_index, xpub_keys, ecdsa, derivation })
    }

    pub async fn try_load(wallet: &Arc<Wallet>, storage: &AccountStorage, meta: Option<Arc<AccountMetadata>>) -> Result<Self> {
        let storable = Storable::try_load(storage)?;
        let prv_key_data_id: PrvKeyDataId = storage.prv_key_data_ids.clone().try_into()?;
        let inner = Arc::new(Inner::from_storage(wallet, storage));

        let Storable { account_index, xpub_keys, ecdsa, .. } = storable;

        let address_derivation_indexes = meta.and_then(|meta| meta.address_derivation_indexes()).unwrap_or_default();

        let derivation = AddressDerivationManager::new(
            wallet,
            BIP32_ACCOUNT_KIND.into(),
            &xpub_keys,
            ecdsa,
            0,
            None,
            1,
            address_derivation_indexes,
        )
        .await?;

        // TODO - is this needed?
        let _prv_key_data_info = wallet
            .store()
            .as_prv_key_data_store()?
            .load_key_info(&prv_key_data_id)
            .await?
            .ok_or_else(|| Error::PrivateKeyNotFound(prv_key_data_id))?;

        Ok(Self { inner, prv_key_data_id, account_index, xpub_keys, ecdsa, derivation })
    }

    pub fn get_address_range_for_scan(&self, range: std::ops::Range<u32>) -> Result<Vec<Address>> {
        let receive_addresses = self.derivation.receive_address_manager().get_range_with_args(range.clone(), false)?;
        let change_addresses = self.derivation.change_address_manager().get_range_with_args(range, false)?;
        Ok(receive_addresses.into_iter().chain(change_addresses).collect::<Vec<_>>())
    }
}

#[async_trait]
impl Account for Bip32 {
    fn inner(&self) -> &Arc<Inner> {
        &self.inner
    }

    fn account_kind(&self) -> AccountKind {
        BIP32_ACCOUNT_KIND.into()
    }

    fn prv_key_data_id(&self) -> Result<&PrvKeyDataId> {
        Ok(&self.prv_key_data_id)
    }

    fn as_dyn_arc(self: Arc<Self>) -> Arc<dyn Account> {
        self
    }

    fn sig_op_count(&self) -> u8 {
        1
    }

    fn minimum_signatures(&self) -> u16 {
        1
    }

    fn receive_address(&self) -> Result<Address> {
        self.derivation.receive_address_manager().current_address()
    }
    fn change_address(&self) -> Result<Address> {
        self.derivation.change_address_manager().current_address()
    }

    fn to_storage(&self) -> Result<AccountStorage> {
        let settings = self.context().settings.clone();
        let storable = Storable::new(self.account_index, self.xpub_keys.clone(), self.ecdsa);
        let serialized = serde_json::to_string(&storable)?;
        let storage = AccountStorage::new(
            BIP32_ACCOUNT_KIND.into(),
            self.id(),
            self.storage_key(),
            self.prv_key_data_id.into(),
            settings,
            serialized.as_bytes(),
        );

        Ok(storage)
    }

    fn metadata(&self) -> Result<Option<AccountMetadata>> {
        let metadata = AccountMetadata::new(self.inner.id, self.derivation.address_derivation_meta());
        Ok(Some(metadata))
    }

    fn descriptor(&self) -> Result<AccountDescriptor> {
        let descriptor = AccountDescriptor::new(
            BIP32_ACCOUNT_KIND.into(),
            *self.id(),
            self.name(),
            self.prv_key_data_id.into(),
            self.receive_address().ok(),
            self.change_address().ok(),
        )
        .with_property(AccountDescriptorProperty::AccountIndex, self.account_index.into())
        .with_property(AccountDescriptorProperty::XpubKeys, self.xpub_keys.clone().into())
        .with_property(AccountDescriptorProperty::Ecdsa, self.ecdsa.into())
        .with_property(AccountDescriptorProperty::DerivationMeta, self.derivation.address_derivation_meta().into());

        Ok(descriptor)
    }

    fn as_derivation_capable(self: Arc<Self>) -> Result<Arc<dyn DerivationCapableAccount>> {
        Ok(self.clone())
    }
}

impl DerivationCapableAccount for Bip32 {
    fn derivation(&self) -> Arc<dyn AddressDerivationManagerTrait> {
        self.derivation.clone()
    }

    fn account_index(&self) -> u64 {
        self.account_index
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::*;

    #[test]
    fn test_storage_bip32() -> Result<()> {
        let storable_in = Storable::new(0xbaadf00d, vec![make_xpub()].into(), false);
        let guard = StorageGuard::new(&storable_in);
        let storable_out = guard.validate()?;

        assert_eq!(storable_in.account_index, storable_out.account_index);
        assert_eq!(storable_in.ecdsa, storable_out.ecdsa);
        assert_eq!(storable_in.xpub_keys.len(), storable_out.xpub_keys.len());
        for idx in 0..storable_in.xpub_keys.len() {
            assert_eq!(storable_in.xpub_keys[idx], storable_out.xpub_keys[idx]);
        }

        Ok(())
    }
}