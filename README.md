[![Build Status](https://travis-ci.org/dpc/mioecho.svg?branch=master)](https://travis-ci.org/dpc/mioecho)

# mioecho

## Introduction

`mioecho` is a simple TCP echo server based on [mio][mio] [rust language][rust] library.

The idea is to write a basic, but polished example for evaluation and reference.

[rust]: http://rust-lang.org
[mio]: https://github.com/carllerche/mio
[hex2d-dpcext-rs]: http://github.com/dpc/hex2d-dpcext-rs

## Building & running

    cargo run --release

# Semi-benchmarks

Be aware: This is amateurish and probably misleading comparison!

Using: https://gist.github.com/dpc/8cacd3b6fa5273ffdcce

```
# mioecho
% GOMAXPROCS=64 ./tcp_bench  -c=128 -t=10  -a="127.0.0.1:5555"
Benchmarking: 127.0.0.1:5555
128 clients, running 26 bytes, 10 sec.

Speed: 178946 request/sec, 178946 response/sec
Requests: 1789460
Responses: 1789460

# server_libev (simple libev based server):
% GOMAXPROCS=64 ./tcp_bench  -c=128 -t=10  -a="127.0.0.1:5000"
Benchmarking: 127.0.0.1:3100
128 clients, running 26 bytes, 10 sec.

Speed: 210485 request/sec, 210485 response/sec
Requests: 2104856
Responses: 2104854
```

Using: https://github.com/dpc/benchmark-echo

```
rust ./mioecho:5555
Throughput: 206731.02 [reqests/sec], errors: 0
Throughput: 191145.35 [reqests/sec], errors: 0
Throughput: 154950.69 [reqests/sec], errors: 0
Throughput: 156119.67 [reqests/sec], errors: 0
Throughput: 176071.46 [reqests/sec], errors: 0

c ./server_libev 3100
Throughput: 192770.52 [reqests/sec], errors: 0
Throughput: 156105.10 [reqests/sec], errors: 0
Throughput: 162632.05 [reqests/sec], errors: 0
Throughput: 179868.05 [reqests/sec], errors: 0
Throughput: 187706.06 [reqests/sec], errors: 0
```
