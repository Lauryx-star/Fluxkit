//! Account-key wrapping and tuple-bound secret encryption; no passwords or keys are serialized.
use super::ServerIdentity;
use aes_gcm::{
    aead::{rand_core::RngCore, Aead, KeyInit, OsRng, Payload},
    Aes256Gcm, Nonce,
};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const INVALID: &str = "The encrypted vault data is invalid or uses unsupported parameters.";
const UNLOCK: &str =
    "The master password is incorrect or the encrypted vault was modified. No values were changed.";
pub type DataKey = Zeroizing<[u8; 32]>;
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Kdf {
    algorithm: String,
    version: u32,
    memory_ki_b: u32,
    iterations: u32,
    parallelism: u32,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Profile {
    schema_version: u32,
    algorithm: String,
    kdf: Kdf,
    salt: String,
    nonce: String,
    wrapped_key: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CipherValue {
    pub variable_id: String,
    pub environment_id: String,
    nonce: String,
    ciphertext: String,
}
fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0; N];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| "Secure randomness is unavailable. No secret was saved.".to_string())?;
    Ok(bytes)
}
fn decode(value: &str, min: usize, max: usize) -> Result<Vec<u8>, String> {
    if value.len() > max.div_ceil(3) * 4 {
        return Err(INVALID.into());
    }
    let bytes = STANDARD.decode(value).map_err(|_| INVALID.to_string())?;
    if !(min..=max).contains(&bytes.len()) {
        return Err(INVALID.into());
    }
    Ok(bytes)
}
fn wrap_aad(owner: &ServerIdentity) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&("fluxkit-account-key-v1", owner.account_id)).map_err(|_| INVALID.into())
}
fn value_aad(
    owner: &ServerIdentity,
    project: &str,
    variable: &str,
    environment: &str,
) -> Result<Vec<u8>, String> {
    uuid::Uuid::parse_str(project).map_err(|_| INVALID.to_string())?;
    uuid::Uuid::parse_str(variable).map_err(|_| INVALID.to_string())?;
    if environment != "default" {
        uuid::Uuid::parse_str(environment).map_err(|_| INVALID.to_string())?;
    }
    serde_json::to_vec(&(
        "fluxkit-secret-v1",
        project,
        owner.account_id,
        variable,
        environment,
    ))
    .map_err(|_| INVALID.into())
}
fn derive(password: &str, salt: &[u8], kdf: &Kdf) -> Result<DataKey, String> {
    if kdf.algorithm != "Argon2id"
        || kdf.version != 19
        || !(65_536..=262_144).contains(&kdf.memory_ki_b)
        || !(3..=10).contains(&kdf.iterations)
        || !(1..=4).contains(&kdf.parallelism)
        || password.len() > 1024
    {
        return Err(INVALID.into());
    }
    let params = Params::new(kdf.memory_ki_b, kdf.iterations, kdf.parallelism, Some(32))
        .map_err(|_| INVALID.to_string())?;
    let mut key = Zeroizing::new([0; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|_| INVALID.to_string())?;
    Ok(key)
}
pub fn create(owner: &ServerIdentity, password: &str) -> Result<(Profile, DataKey), String> {
    let key = Zeroizing::new(random()?);
    Ok((rewrap(owner, password, &key)?, key))
}
pub fn rewrap(owner: &ServerIdentity, password: &str, key: &DataKey) -> Result<Profile, String> {
    if !(12..=1024).contains(&password.len()) {
        return Err("Use a master password of 12 to 1,024 UTF-8 bytes.".into());
    }
    let salt = random::<16>()?;
    let nonce = random::<12>()?;
    let kdf = Kdf {
        algorithm: "Argon2id".into(),
        version: 19,
        memory_ki_b: 65_536,
        iterations: 3,
        parallelism: 1,
    };
    let wrapping_key = derive(password, &salt, &kdf)?;
    let cipher =
        Aes256Gcm::new_from_slice(wrapping_key.as_ref()).map_err(|_| INVALID.to_string())?;
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: key.as_ref(),
                aad: &wrap_aad(owner)?,
            },
        )
        .map_err(|_| INVALID.to_string())?;
    Ok(Profile {
        schema_version: 1,
        algorithm: "AES-256-GCM".into(),
        kdf,
        salt: STANDARD.encode(salt),
        nonce: STANDARD.encode(nonce),
        wrapped_key: STANDARD.encode(ciphertext),
    })
}
pub fn unlock(
    owner: &ServerIdentity,
    password: &str,
    profile: &Profile,
) -> Result<DataKey, String> {
    if profile.schema_version != 1 || profile.algorithm != "AES-256-GCM" {
        return Err(INVALID.into());
    }
    let salt = decode(&profile.salt, 16, 16)?;
    let nonce = decode(&profile.nonce, 12, 12)?;
    let wrapped = decode(&profile.wrapped_key, 48, 48)?;
    let wrapping_key = derive(password, &salt, &profile.kdf)?;
    let cipher =
        Aes256Gcm::new_from_slice(wrapping_key.as_ref()).map_err(|_| INVALID.to_string())?;
    let bytes = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &wrapped,
                    aad: &wrap_aad(owner)?,
                },
            )
            .map_err(|_| UNLOCK.to_string())?,
    );
    let mut key = Zeroizing::new([0; 32]);
    if bytes.len() != 32 {
        return Err(INVALID.into());
    }
    key.copy_from_slice(&bytes);
    Ok(key)
}
pub fn encrypt(
    owner: &ServerIdentity,
    project: &str,
    variable: &str,
    environment: &str,
    value: &str,
    key: &DataKey,
) -> Result<CipherValue, String> {
    if value.len() > 2560 {
        return Err("Values must not exceed 2,560 UTF-8 bytes.".into());
    }
    let nonce = random::<12>()?;
    let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| INVALID.to_string())?;
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: value.as_bytes(),
                aad: &value_aad(owner, project, variable, environment)?,
            },
        )
        .map_err(|_| INVALID.to_string())?;
    Ok(CipherValue {
        variable_id: variable.into(),
        environment_id: environment.into(),
        nonce: STANDARD.encode(nonce),
        ciphertext: STANDARD.encode(ciphertext),
    })
}
pub fn decrypt(
    owner: &ServerIdentity,
    project: &str,
    value: &CipherValue,
    key: &DataKey,
) -> Result<Zeroizing<String>, String> {
    let nonce = decode(&value.nonce, 12, 12)?;
    let ciphertext = decode(&value.ciphertext, 16, 2576)?;
    let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| INVALID.to_string())?;
    let bytes = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &value_aad(owner, project, &value.variable_id, &value.environment_id)?,
                },
            )
            .map_err(|_| {
                "The encrypted value could not be authenticated. No value was used.".to_string()
            })?,
    );
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| INVALID.to_string())?
        .to_string();
    Ok(Zeroizing::new(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner() -> ServerIdentity {
        ServerIdentity {
            server_url: "https://synthetic.example.test".into(),
            account_id: uuid::Uuid::new_v4(),
        }
    }
    #[test]
    fn passwords_wrap_a_random_key_and_rewrap_without_reencrypting_values() {
        let owner = owner();
        let started = std::time::Instant::now();
        let (profile, key) = create(&owner, "Synthetic master password 42").unwrap();
        eprintln!(
            "Observed debug vault setup (64 MiB, 3 iterations, 1 lane): {} ms",
            started.elapsed().as_millis()
        );
        let unlocked = unlock(&owner, "Synthetic master password 42", &profile).unwrap();
        assert_eq!(key.as_ref(), unlocked.as_ref());
        let project = uuid::Uuid::new_v4().to_string();
        let variable = uuid::Uuid::new_v4().to_string();
        let secret = encrypt(
            &owner,
            &project,
            &variable,
            "default",
            "Synthetic secret",
            &key,
        )
        .unwrap();
        assert!(unlock(&owner, "Wrong password", &profile).is_err());
        let changed = rewrap(&owner, "Another synthetic master password", &key).unwrap();
        assert!(unlock(&owner, "Synthetic master password 42", &changed).is_err());
        let restored = unlock(&owner, "Another synthetic master password", &changed).unwrap();
        assert_eq!(
            &*decrypt(&owner, &project, &secret, &restored).unwrap(),
            "Synthetic secret"
        );
        assert!(!serde_json::to_string(&profile)
            .unwrap()
            .contains("Synthetic"));
    }
    #[test]
    fn authenticated_data_blocks_cross_account_project_variable_environment_and_ciphertext_reuse() {
        let owner = owner();
        let key = Zeroizing::new(random().unwrap());
        let project = uuid::Uuid::new_v4().to_string();
        let variable = uuid::Uuid::new_v4().to_string();
        let value = encrypt(&owner, &project, &variable, "default", "", &key).unwrap();
        assert_eq!(&*decrypt(&owner, &project, &value, &key).unwrap(), "");
        let other = ServerIdentity {
            account_id: uuid::Uuid::new_v4(),
            ..owner.clone()
        };
        assert!(decrypt(&other, &project, &value, &key).is_err());
        assert!(decrypt(&owner, &uuid::Uuid::new_v4().to_string(), &value, &key).is_err());
        let mut changed = value.clone();
        changed.variable_id = uuid::Uuid::new_v4().to_string();
        assert!(decrypt(&owner, &project, &changed, &key).is_err());
        changed = value.clone();
        changed.environment_id = uuid::Uuid::new_v4().to_string();
        assert!(decrypt(&owner, &project, &changed, &key).is_err());
        changed = value;
        changed.ciphertext = STANDARD.encode([0; 16]);
        assert!(decrypt(&owner, &project, &changed, &key).is_err());
    }
    #[test]
    fn untrusted_kdf_cost_and_nonce_lengths_are_rejected_before_expensive_work() {
        let mut profile = Profile {
            schema_version: 1,
            algorithm: "AES-256-GCM".into(),
            kdf: Kdf {
                algorithm: "Argon2id".into(),
                version: 19,
                memory_ki_b: u32::MAX,
                iterations: 3,
                parallelism: 1,
            },
            salt: STANDARD.encode([0; 16]),
            nonce: STANDARD.encode([0; 12]),
            wrapped_key: STANDARD.encode([0; 48]),
        };
        assert!(unlock(&owner(), "Synthetic master password", &profile).is_err());
        profile.kdf.memory_ki_b = 65_536;
        profile.nonce = STANDARD.encode([0; 13]);
        assert!(unlock(&owner(), "Synthetic master password", &profile).is_err());
    }
}
