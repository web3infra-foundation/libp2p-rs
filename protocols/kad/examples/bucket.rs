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

// This test is to generate many random peers to fit into k-buckets. We shall
// see it is very hard to get the closer peers...

use libp2prs_core::PeerId;
use libp2prs_kad::kbucket::{Entry, KBucketsTable, Key};

fn main() {
    let local_key = Key::from(PeerId::random());
    let mut table = KBucketsTable::<Key<PeerId>, ()>::new(local_key);
    let mut count = 0;
    let mut lc = 0;
    loop {
        lc += 1;
        if count == 1000 {
            break;
        }

        if lc % 100000 == 0 {
            println!("lc={} c={}", lc, count);
            print_table(&mut table);
        }
        let key = Key::from(PeerId::random());
        if let Entry::Absent(mut e) = table.entry(&key) {
            if e.insert(()) {
                count += 1
            } else {
                continue;
            }
        } else {
            panic!("entry exists")
        }
    }
}

fn print_table(table: &mut KBucketsTable<Key<PeerId>, ()>) {
    let view = table
        .iter()
        .filter(|k| !k.is_empty())
        .map(|k| {
            let index = k.index();

            (index, k.num_entries())
        })
        .collect::<Vec<_>>();

    println!("{:?}", view);
}
