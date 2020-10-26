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

 package main

 import (
	 "fmt"
	 "net"
	 "context"
	 "io"
	 ci "github.com/libp2p/go-libp2p-core/crypto"
	 "github.com/libp2p/go-libp2p-core/peer"
	 "github.com/libp2p/go-libp2p-core/sec"
	 secio "github.com/libp2p/go-libp2p-secio"
 )
 

 
 func main() {
		serverTpt := newTransport(ci.Secp256k1, 32)
		lstnr, err := net.Listen("tcp", "0.0.0.0:1337")
		if err != nil {
			panic(err)
		}
		for {
			server, err := lstnr.Accept()
			if err != nil {
				panic(err)
			}
			serverConn, err := serverTpt.SecureInbound(context.TODO(), server)
			fmt.Println("local id",serverTpt.LocalID)
			if err != nil {
				panic(err)
			}
			go handleClient(serverConn)
		}	
 }

 type keyInfo struct {
	cipherKey []byte
	iv        []byte
	macKey    []byte
}




 func handleClient(conn sec.SecureConn) {
	defer conn.Close()
    buf := make([]byte, 256)     // using small tmo buffer for demonstrating
    for {
        n, err := conn.Read(buf)
        if err != nil {
            if err != io.EOF {
                fmt.Println("read error:", err)
            }
            break
        }
		fmt.Println("got", n, "bytes.")
		fmt.Println("buf:", buf)
		fmt.Println("rev msg:", string(buf[:n]))
		conn.Write(buf[:n])

	}
}

 func newTransport(typ, bits int) *secio.Transport {
	priv, pub, err := ci.GenerateKeyPair(typ, bits)
	fmt.Println("priv",priv)
	if err != nil {
		panic(err)
	}
	id, err := peer.IDFromPublicKey(pub)
	if err != nil {
		panic(err)
	}
	return &secio.Transport{
		PrivateKey: priv,
		LocalID:    id,
	}
}

 