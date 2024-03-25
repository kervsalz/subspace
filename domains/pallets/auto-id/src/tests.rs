use crate::pallet::{AutoIds, NextAutoIdIdentifier};
use crate::{
    self as pallet_auto_id, Identifier, Pallet, RegisterAutoId, RegisterAutoIdX509, Signature,
};
use codec::Encode;
use frame_support::dispatch::RawOrigin;
use frame_support::traits::{ConstU16, ConstU32, ConstU64, Time};
use pem::parse;
use ring::rand::SystemRandom;
use ring::signature::RsaKeyPair;
use sp_certificate_registry::DerVec;
use sp_core::H256;
use sp_runtime::traits::{BlakeTwo256, IdentityLookup};
use sp_runtime::BuildStorage;
use std::sync::Arc;
use subspace_runtime_primitives::Moment;
use x509_parser::der_parser::asn1_rs::ToDer;
use x509_parser::oid_registry::OID_PKCS1_SHA256WITHRSA;
use x509_parser::prelude::{AlgorithmIdentifier, FromDer};

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
    pub struct Test {
        System: frame_system,
        AutoId: pallet_auto_id,
    }
);

pub struct MockTime;
impl Time for MockTime {
    type Moment = Moment;

    fn now() -> Self::Moment {
        // valid block time for testing certs
        1_711_367_658_200
    }
}

impl pallet_auto_id::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type Time = MockTime;
}

impl frame_system::Config for Test {
    type BaseCallFilter = frame_support::traits::Everything;
    type BlockWeights = ();
    type BlockLength = ();
    type DbWeight = ();
    type RuntimeOrigin = RuntimeOrigin;
    type RuntimeCall = RuntimeCall;
    type RuntimeTask = RuntimeTask;
    type Nonce = u64;
    type Hash = H256;
    type Hashing = BlakeTwo256;
    type AccountId = u64;
    type Lookup = IdentityLookup<Self::AccountId>;
    type Block = Block;
    type RuntimeEvent = RuntimeEvent;
    type BlockHashCount = ConstU64<250>;
    type Version = ();
    type PalletInfo = PalletInfo;
    type AccountData = ();
    type OnNewAccount = ();
    type OnKilledAccount = ();
    type SystemWeightInfo = ();
    type SS58Prefix = ConstU16<42>;
    type OnSetCode = ();
    type MaxConsumers = ConstU32<16>;
}

pub(crate) fn new_test_ext() -> sp_io::TestExternalities {
    let t = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();

    let mut ext: sp_io::TestExternalities = t.into();
    ext.register_extension(
        sp_certificate_registry::host_functions::HostFunctionExtension::new(Arc::new(
            sp_certificate_registry::host_functions::HostFunctionsImpl,
        )),
    );
    ext
}

/// Converts Algorithm identifier to Der since x509 does not implement the ToDer :(.
fn algorithm_to_der(algorithm_identifier: AlgorithmIdentifier) -> DerVec {
    let sequence_tag: u8 = 0x30;
    let sequence_content = {
        let mut temp = Vec::new();
        temp.extend(algorithm_identifier.algorithm.to_der_vec().unwrap());
        temp.extend(algorithm_identifier.parameters.to_der_vec().unwrap());
        temp
    };
    let encoded_sequence_length = {
        let content_length = sequence_content.len();
        if content_length > 127 {
            // This is long form length encoding
            let length_as_bytes = content_length.to_be_bytes();
            let mut encoded = Vec::with_capacity(length_as_bytes.len() + 1);
            // Set first bit to 1 and store number of length-bytes.
            encoded.push(0x80 | (length_as_bytes.len() as u8));
            encoded.extend_from_slice(&length_as_bytes);
            encoded
        } else {
            // The short form (single-byte length) can be used.
            vec![content_length as u8]
        }
    };

    let mut d = Vec::new();
    d.push(sequence_tag);
    d.extend(encoded_sequence_length);
    d.extend(sequence_content);
    let (_, derived) = AlgorithmIdentifier::from_der(&d).unwrap();
    assert_eq!(algorithm_identifier, derived);
    d.into()
}

fn register_issuer_auto_id() -> Identifier {
    let issuer_cert = include_bytes!("../res/issuer.cert.der").to_vec();
    let (_, cert) = x509_parser::certificate::X509Certificate::from_der(&issuer_cert).unwrap();

    let auto_id_identifier = NextAutoIdIdentifier::<Test>::get();
    Pallet::<Test>::register_auto_id(
        RawOrigin::Signed(1).into(),
        RegisterAutoId::X509(RegisterAutoIdX509::Root {
            certificate: cert.tbs_certificate.as_ref().to_vec().into(),
            signature_algorithm: algorithm_to_der(cert.signature_algorithm),
            signature: cert.signature_value.as_ref().to_vec(),
        }),
    )
    .unwrap();

    assert_eq!(NextAutoIdIdentifier::<Test>::get(), auto_id_identifier + 1);
    auto_id_identifier
}

fn register_leaf_auto_id(issuer_auto_id: Identifier) -> Identifier {
    let cert = include_bytes!("../res/leaf.cert.der").to_vec();
    let (_, cert) = x509_parser::certificate::X509Certificate::from_der(&cert).unwrap();
    let auto_id_identifier = NextAutoIdIdentifier::<Test>::get();
    Pallet::<Test>::register_auto_id(
        RawOrigin::Signed(1).into(),
        RegisterAutoId::X509(RegisterAutoIdX509::Leaf {
            issuer_id: issuer_auto_id,
            certificate: cert.tbs_certificate.as_ref().to_vec().into(),
            signature_algorithm: algorithm_to_der(cert.signature_algorithm),
            signature: cert.signature_value.as_ref().to_vec(),
        }),
    )
    .unwrap();

    assert_eq!(NextAutoIdIdentifier::<Test>::get(), auto_id_identifier + 1);
    auto_id_identifier
}

fn sign_preimage(data: Vec<u8>) -> Signature {
    let priv_key_pem = include_str!("../res/private.issuer.pem");
    let priv_key_der = parse(priv_key_pem).unwrap().contents().to_vec();
    let rsa_key_pair = RsaKeyPair::from_pkcs8(&priv_key_der).unwrap();
    let mut signature = vec![0; rsa_key_pair.public().modulus_len()];
    let rng = SystemRandom::new();
    rsa_key_pair
        .sign(
            &ring::signature::RSA_PKCS1_SHA256,
            &rng,
            &data,
            &mut signature,
        )
        .unwrap();
    let algo = AlgorithmIdentifier {
        algorithm: OID_PKCS1_SHA256WITHRSA,
        parameters: None,
    };
    Signature {
        signature_algorithm: algorithm_to_der(algo),
        value: signature,
    }
}

#[test]
fn test_register_issuer_auto_id() {
    new_test_ext().execute_with(|| {
        register_issuer_auto_id();
    })
}

#[test]
fn test_register_leaf_auto_id() {
    new_test_ext().execute_with(|| {
        let issuer_id = register_issuer_auto_id();
        register_leaf_auto_id(issuer_id);
    })
}

#[test]
fn test_revoke_certificate() {
    new_test_ext().execute_with(|| {
        let auto_id_identifier = register_issuer_auto_id();
        let auto_id = AutoIds::<Test>::get(auto_id_identifier).unwrap();
        assert!(!auto_id.certificate.is_revoked());
        let signature = sign_preimage(auto_id_identifier.encode());
        Pallet::<Test>::revoke_certificate(
            RawOrigin::Signed(1).into(),
            auto_id_identifier,
            signature,
        )
        .unwrap();
        let auto_id = AutoIds::<Test>::get(auto_id_identifier).unwrap();
        assert!(auto_id.certificate.is_revoked());
    })
}

#[test]
fn test_deactivate_auto_id() {
    new_test_ext().execute_with(|| {
        let auto_id_identifier = register_issuer_auto_id();
        let signature = sign_preimage(auto_id_identifier.encode());
        Pallet::<Test>::deactivate_auto_id(
            RawOrigin::Signed(1).into(),
            auto_id_identifier,
            signature,
        )
        .unwrap();
        assert!(AutoIds::<Test>::get(auto_id_identifier).is_none());
    })
}
