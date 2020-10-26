# Security Stream

Security stream provides the secure session over the underlying I/O connection. Data will be encrypted to transmit in this stream.
Now we have three different security stream implementations:

### Secio
Secio generates the public/private key pair based on asymmetric cryptographic algorithm.
In this part we provide RSA, ed25519 and secp256k1.
When both parties use secio to establish a security stream, the following steps will be executed:
1. Combine a handshake package which contains pubkey, nonce, supported elliptic curve algorithm, symmetric encryption algorithm, hash algorithm. 
Then send it to the other party.
2. Receive the package and confirm algorithms that both intend to use.
3. Generate a ephemeral public/private key by elliptic curve algorithm.
The ephemeral public key, received and send package are encrypted by private key and send it as the second handshake package to the opposite.
4. Use opposite's public key to check the received package.
5. Ensure the symmetric encryption algorithm and hmac, verify the nonce correctly or not.
6. Finally use encryption and hmac to transmit.


### PlainText
The implementation of plaintext is simple as it doesn't perform the security operations at all. 

Both sides exchange theirs public key, and verify it that matches local key or not.
If result is different, it means that connection has been established.
And then use plaintext to transmit.


### Noise
Noise is more complicated than both secio and plaintext.
Now we only provide `xx` pattern but it has 12 kinds of pattern actually.
The implementation refers to `go-libp2p` and `rust-libp2p`, but we use `async/await` to instead of `poll`.

Noise use the crate `snow` to implement the real noise protocol. 
In this part, we encrypt public key and combine it with private key as a package.
Then send it to `snow`.