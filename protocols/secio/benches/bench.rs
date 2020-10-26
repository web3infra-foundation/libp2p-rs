// Copyright 2020 Netwarps Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use criterion::{criterion_group, criterion_main, Bencher, Criterion};
use libp2prs_secio::{
    codec::Hmac,
    crypto::{cipher::CipherType, new_stream, CryptoMode},
};

fn decode_encode(data: &[u8], cipher: CipherType) {
    let cipher_key = (0..cipher.key_size()).map(|_| rand::random::<u8>()).collect::<Vec<_>>();
    let _hmac_key: [u8; 32] = rand::random();
    let iv = (0..cipher.iv_size()).map(|_| rand::random::<u8>()).collect::<Vec<_>>();

    let mut encode_cipher = new_stream(cipher, &cipher_key, &iv, CryptoMode::Encrypt);
    let mut decode_cipher = new_stream(cipher, &cipher_key, &iv, CryptoMode::Decrypt);
    let (mut decode_hmac, mut encode_hmac): (Option<Hmac>, Option<Hmac>) = match cipher {
        CipherType::ChaCha20Poly1305 | CipherType::Aes128Gcm | CipherType::Aes256Gcm => (None, None),
        // #[cfg(unix)]
        _ => {
            use libp2prs_secio::Digest;
            let encode_hmac = Hmac::from_key(Digest::Sha256, &_hmac_key);
            let decode_hmac = encode_hmac.clone();
            (Some(decode_hmac), Some(encode_hmac))
        }
    };

    let mut encode_data = encode_cipher.encrypt(&data[..]).unwrap();
    if encode_hmac.is_some() {
        let signature = encode_hmac.as_mut().unwrap().sign(&encode_data[..]);
        encode_data.extend_from_slice(signature.as_ref());
    }

    if decode_hmac.is_some() {
        let content_length = encode_data.len() - decode_hmac.as_mut().unwrap().num_bytes();

        let (crypted_data, expected_hash) = encode_data.split_at(content_length);

        assert!(decode_hmac.as_mut().unwrap().verify(crypted_data, expected_hash));

        encode_data.truncate(content_length);
    }

    let decode_data = decode_cipher.decrypt(&encode_data).unwrap();

    assert_eq!(&decode_data[..], &data[..]);
}

fn bench_test(bench: &mut Bencher, cipher: CipherType, data: &[u8]) {
    bench.iter(|| {
        decode_encode(data, cipher);
    })
}

fn criterion_benchmark(bench: &mut Criterion) {
    let data = (0..1024 * 256).map(|_| rand::random::<u8>()).collect::<Vec<_>>();
    // #[cfg(unix)]
    bench.bench_function("1kb_aes128ctr", {
        let data = data.clone();
        move |b| bench_test(b, CipherType::Aes128Ctr, &data)
    });
    // #[cfg(unix)]
    // bench.bench_function("1kb_aes256ctr", {
    //     let data = data.clone();
    //     move |b| bench_test(b, CipherType::Aes256Ctr, &data)
    // });
    bench.bench_function("1kb_aes128gcm", {
        let data = data.clone();
        move |b| bench_test(b, CipherType::Aes128Gcm, &data)
    });
    bench.bench_function("1kb_aes256gcm", {
        let data = data.clone();
        move |b| bench_test(b, CipherType::Aes256Gcm, &data)
    });
    bench.bench_function("1kb_chacha20poly1305", move |b| bench_test(b, CipherType::ChaCha20Poly1305, &data));

    let data = (0..1024 * 1024).map(|_| rand::random::<u8>()).collect::<Vec<_>>();
    // #[cfg(unix)]
    bench.bench_function("1mb_aes128ctr", {
        let data = data.clone();
        move |b| bench_test(b, CipherType::Aes128Ctr, &data)
    });
    // #[cfg(unix)]
    // bench.bench_function("1mb_aes256ctr", {
    //     let data = data.clone();
    //     move |b| bench_test(b, CipherType::Aes256Ctr, &data)
    // });
    bench.bench_function("1mb_aes128gcm", {
        let data = data.clone();
        move |b| bench_test(b, CipherType::Aes128Gcm, &data)
    });
    bench.bench_function("1mb_aes256gcm", {
        let data = data.clone();
        move |b| bench_test(b, CipherType::Aes256Gcm, &data)
    });
    bench.bench_function("1mb_chacha20poly1305", move |b| bench_test(b, CipherType::ChaCha20Poly1305, &data));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
