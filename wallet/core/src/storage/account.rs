use crate::imports::*;
use crate::result::Result;
use crate::storage::PrvKeyDataId;

const ACCOUNT_SETTINGS_VERSION: u32 = 0;

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub struct AccountSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Vec<u8>>,
}

impl BorshSerialize for AccountSettings {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        BorshSerialize::serialize(&ACCOUNT_SETTINGS_VERSION, writer)?;
        BorshSerialize::serialize(&self.name, writer)?;
        BorshSerialize::serialize(&self.meta, writer)?;

        Ok(())
    }
}

impl BorshDeserialize for AccountSettings {
    fn deserialize(buf: &mut &[u8]) -> IoResult<Self> {
        let _version: u32 = BorshDeserialize::deserialize(buf)?;
        let name = BorshDeserialize::deserialize(buf)?;
        let meta = BorshDeserialize::deserialize(buf)?;

        Ok(Self { name, meta })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[serde(rename_all = "lowercase")]
#[serde(tag = "type", content = "data")]
pub enum AssocPrvKeyDataIds {
    None,
    Single(PrvKeyDataId),
    Multiple(Arc<Vec<PrvKeyDataId>>),
}

impl From<PrvKeyDataId> for AssocPrvKeyDataIds {
    fn from(value: PrvKeyDataId) -> Self {
        AssocPrvKeyDataIds::Single(value)
    }
}

impl TryFrom<Option<Arc<Vec<PrvKeyDataId>>>> for AssocPrvKeyDataIds {
    type Error = Error;

    fn try_from(value: Option<Arc<Vec<PrvKeyDataId>>>) -> Result<Self> {
        match value {
            None => Ok(AssocPrvKeyDataIds::None),
            Some(ids) => {
                if ids.is_empty() {
                    return Err(Error::AssocPrvKeyDataIdsEmpty);
                }
                Ok(AssocPrvKeyDataIds::Multiple(ids))
            }
        }
    }
}

impl TryFrom<AssocPrvKeyDataIds> for PrvKeyDataId {
    type Error = Error;

    fn try_from(value: AssocPrvKeyDataIds) -> Result<Self> {
        match value {
            AssocPrvKeyDataIds::Single(id) => Ok(id),
            _ => Err(Error::AssocPrvKeyDataIds("Single".to_string(), value)),
        }
    }
}

impl TryFrom<AssocPrvKeyDataIds> for Arc<Vec<PrvKeyDataId>> {
    type Error = Error;

    fn try_from(value: AssocPrvKeyDataIds) -> Result<Self> {
        match value {
            AssocPrvKeyDataIds::Multiple(ids) => Ok(ids),
            _ => Err(Error::AssocPrvKeyDataIds("Multiple".to_string(), value)),
        }
    }
}

impl TryFrom<AssocPrvKeyDataIds> for Option<Arc<Vec<PrvKeyDataId>>> {
    type Error = Error;

    fn try_from(value: AssocPrvKeyDataIds) -> Result<Self> {
        match value {
            AssocPrvKeyDataIds::None => Ok(None),
            AssocPrvKeyDataIds::Multiple(ids) => Ok(Some(ids)),
            _ => Err(Error::AssocPrvKeyDataIds("None or Multiple".to_string(), value)),
        }
    }
}

impl AssocPrvKeyDataIds {
    pub fn contains(&self, id: &PrvKeyDataId) -> bool {
        match self {
            AssocPrvKeyDataIds::None => false,
            AssocPrvKeyDataIds::Single(single) => single == id,
            AssocPrvKeyDataIds::Multiple(multiple) => multiple.iter().any(|elem| elem == id),
        }
    }
}

const ACCOUNT_MAGIC: u32 = 0x4B415341;
const ACCOUNT_VERSION: u32 = 0;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountStorage {
    pub kind: AccountKind,
    pub id: AccountId,
    pub storage_key: AccountStorageKey,
    pub prv_key_data_ids: AssocPrvKeyDataIds,
    pub settings: AccountSettings,
    pub serialized: Vec<u8>,
}

impl AccountStorage {
    pub fn new(
        kind: AccountKind,
        id: &AccountId,
        storage_key: &AccountStorageKey,
        prv_key_data_ids: AssocPrvKeyDataIds,
        settings: AccountSettings,
        serialized: &[u8],
    ) -> Self {
        Self { id: *id, storage_key: *storage_key, kind, prv_key_data_ids, settings, serialized: serialized.to_vec() }
    }

    pub fn id(&self) -> &AccountId {
        &self.id
    }

    pub fn storage_key(&self) -> &AccountStorageKey {
        &self.storage_key
    }

    pub fn serialized(&self) -> &[u8] {
        &self.serialized
    }
}

impl BorshSerialize for AccountStorage {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        StorageHeader::new(ACCOUNT_MAGIC, ACCOUNT_VERSION).serialize(writer)?;
        BorshSerialize::serialize(&self.kind, writer)?;
        BorshSerialize::serialize(&self.id, writer)?;
        BorshSerialize::serialize(&self.storage_key, writer)?;
        BorshSerialize::serialize(&self.prv_key_data_ids, writer)?;
        BorshSerialize::serialize(&self.settings, writer)?;
        BorshSerialize::serialize(&self.serialized, writer)?;

        Ok(())
    }
}

impl BorshDeserialize for AccountStorage {
    fn deserialize(buf: &mut &[u8]) -> IoResult<Self> {
        let StorageHeader { version: _, .. } =
            StorageHeader::deserialize(buf)?.try_magic(ACCOUNT_MAGIC)?.try_version(ACCOUNT_VERSION)?;

        let kind = BorshDeserialize::deserialize(buf)?;
        let id = BorshDeserialize::deserialize(buf)?;
        let storage_key = BorshDeserialize::deserialize(buf)?;
        let prv_key_data_ids = BorshDeserialize::deserialize(buf)?;
        let settings = BorshDeserialize::deserialize(buf)?;
        let serialized = BorshDeserialize::deserialize(buf)?;

        Ok(Self { kind, id, storage_key, prv_key_data_ids, settings, serialized })
    }
}
