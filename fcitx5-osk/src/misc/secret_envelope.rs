use anyhow::Result;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use openssl::{
    ec::{EcGroup, EcKey},
    encrypt::Decrypter,
    hash::MessageDigest,
    nid::Nid,
    pkey::{PKey, Private},
    rsa::Padding,
    sign::Signer,
    symm::{Cipher, Crypter, Mode},
};

const RSA_ALGORITHM: &str = "rsa-oaep-sha256";
const EC_ALGORITHM: &str = "ecdh-aes-256-gcm";

pub fn new_private_key() -> Result<PKey<Private>> {
    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1)?;
    let ec_key = EcKey::generate(&group)?;
    Ok(PKey::from_ec_key(ec_key)?)
}

pub fn decrypt(private_key: &PKey<Private>, secret: &str) -> Result<Vec<u8>> {
    let parts = secret.split('.').collect::<Vec<_>>();
    if parts.first() != Some(&"v1") {
        anyhow::bail!("unsupported envelope version");
    }

    match parts.get(1).copied() {
        Some(RSA_ALGORITHM) => decrypt_rsa(private_key, &parts),
        Some(EC_ALGORITHM) => decrypt_ec(private_key, &parts),
        _ => anyhow::bail!("unsupported envelope algorithm: {:?}", parts.get(1)),
    }
}

fn decrypt_rsa(private_key: &PKey<Private>, parts: &[&str]) -> Result<Vec<u8>> {
    if parts.len() != 4 {
        anyhow::bail!("invalid RSA envelope, parts: {}", parts.len());
    }

    let _info = decode_part(parts[2])?;

    let ciphertext = decode_part(parts[3])?;
    let mut decrypter = Decrypter::new(private_key)?;
    decrypter.set_rsa_padding(Padding::PKCS1_OAEP)?;
    decrypter.set_rsa_oaep_md(MessageDigest::sha256())?;
    decrypter.set_rsa_mgf1_md(MessageDigest::sha256())?;

    let mut plaintext = vec![0; decrypter.decrypt_len(&ciphertext)?];
    let len = decrypter.decrypt(&ciphertext, &mut plaintext)?;
    plaintext.truncate(len);
    Ok(plaintext)
}

fn decrypt_ec(private_key: &PKey<Private>, parts: &[&str]) -> Result<Vec<u8>> {
    if parts.len() != 6 {
        anyhow::bail!("invalid EC envelope, parts: {}", parts.len());
    }

    let info = decode_part(parts[2])?;

    let ephemeral_public_key = decode_part(parts[3])?;
    let iv = decode_part(parts[4])?;
    let ciphertext_and_tag = decode_part(parts[5])?;
    if ciphertext_and_tag.len() < 16 {
        anyhow::bail!(
            "invalid EC ciphertext, it's too small: {}",
            ciphertext_and_tag.len()
        );
    }

    let peer_key = PKey::public_key_from_der(&ephemeral_public_key)?;
    let mut deriver = openssl::derive::Deriver::new(private_key)?;
    deriver.set_peer(&peer_key)?;
    let shared_secret = deriver.derive_to_vec()?;
    let aes_key = derive_ec_aes_key(&shared_secret, &ephemeral_public_key, &info)?;

    let tag_start = ciphertext_and_tag.len() - 16;
    let ciphertext = &ciphertext_and_tag[..tag_start];
    let tag = &ciphertext_and_tag[tag_start..];
    let mut crypter = Crypter::new(Cipher::aes_256_gcm(), Mode::Decrypt, &aes_key, Some(&iv))?;
    crypter.aad_update(&info)?;
    crypter.set_tag(tag)?;

    let mut plaintext = vec![0; ciphertext.len() + Cipher::aes_256_gcm().block_size()];
    let mut len = crypter.update(ciphertext, &mut plaintext)?;
    len += crypter.finalize(&mut plaintext[len..])?;
    plaintext.truncate(len);
    Ok(plaintext)
}

fn derive_ec_aes_key(
    shared_secret: &[u8],
    ephemeral_public_key: &[u8],
    info: &[u8],
) -> Result<Vec<u8>> {
    let pseudo_random_key = hmac_sha256(ephemeral_public_key, shared_secret)?;
    let mut expand_input = Vec::with_capacity(info.len() + 1);
    expand_input.extend_from_slice(info);
    expand_input.push(1);

    hmac_sha256(&pseudo_random_key, &expand_input)
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let key = PKey::hmac(key)?;
    let mut signer = Signer::new(MessageDigest::sha256(), &key)?;
    signer.update(data)?;
    Ok(signer.sign_to_vec()?)
}

fn decode_part(part: &str) -> Result<Vec<u8>> {
    Ok(URL_SAFE_NO_PAD.decode(part)?)
}
