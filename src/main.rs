extern crate mio;
extern crate nix;
extern crate bytes;

use mio::*;
use mio::tcp::*;
use bytes::{RingBuf, Buf, MutBuf};
use mio::util::Slab;
use std::io;
use std::fmt;
use std::fmt::Debug;
use std::fmt::Display;

use std::net::SocketAddr;
use std::str::FromStr;

const SERVER: Token = Token(0);
const CONN_TOKEN_START : Token = Token(1);
const CONN_BUFF_SIZE : usize = 16 * 1024;
const CONNS_MAX : usize = 256;
const DEFAULT_LISTEN_ADDR : &'static str= "127.0.0.1:5555";


fn listend_addr() -> SocketAddr {
    FromStr::from_str(DEFAULT_LISTEN_ADDR).unwrap()
}

struct Connection {
    sock: TcpStream,
    buf: RingBuf,
    token: Option<Token>,
    peer_hup: bool,
    interest: EventSet,
}

impl Connection {
    fn new(sock: TcpStream) -> Connection {
        Connection {
            sock: sock,
            buf: RingBuf::new(CONN_BUFF_SIZE),
            token: None,
            peer_hup: false,
            interest: EventSet::hup() | EventSet::readable(),
        }
    }

    fn is_finished(&self) -> bool {
        self.interest == EventSet::none()
    }

    fn reregister(&mut self,
                  event_loop: &mut EventLoop<Server>,
                 ) -> io::Result<()> {

        // have somewhere to write
        if Buf::remaining(&self.buf) > 0 {
            self.interest.insert(EventSet::writable());
        } else {
            self.interest.remove(EventSet::writable());
        }

        // have somewhere to read to and someone to receive from
        if !self.peer_hup && MutBuf::remaining(&self.buf) > 0 {
            self.interest.insert(EventSet::readable());
        } else {
            self.interest.remove(EventSet::readable());
        }

        event_loop.reregister(
                &self.sock, self.token.unwrap(),
                self.interest, PollOpt::edge() | PollOpt::oneshot()
                )
    }

    fn writable(&mut self,
                event_loop: &mut EventLoop<Server>,
                ) -> io::Result<()> {
        loop {
            let (len, res) = {
                let buf = &self.buf.bytes();
                let len = buf.len();
                let res = self.sock.try_write(buf);
                (len, res)
            };
            match res {
                Ok(None) => {
                    break;
                },
                Ok(Some(r)) => {
                    Buf::advance(&mut self.buf, r);
                    if r != len || Buf::remaining(&self.buf) == 0 {
                        break;
                    }
                },
                Err(_) => {
                    Buf::advance(&mut self.buf, len);
                    self.peer_hup = true;
                    break;
                },
            }
        }

        self.reregister(event_loop)
    }

    fn readable(&mut self,
                event_loop: &mut EventLoop<Server>,
                ) -> io::Result<()> {
        loop {
            let (len, res) = {
                let mut buf = unsafe { &mut self.buf.mut_bytes() };
                let len = buf.len();
                let res = self.sock.try_read(buf);
                (len, res)
            };
            match res {
                Ok(None) => {
                    break;
                },
                Ok(Some(r)) => {
                    unsafe { MutBuf::advance(&mut self.buf, r) };
                    if r != len || MutBuf::remaining(&self.buf) == 0 {
                        break;
                    }
                },
                Err(_) => {
                    break;
                }
            };
        }

        self.reregister(event_loop)
    }

    fn hup(&mut self,
           event_loop: &mut EventLoop<Server>,
          ) -> io::Result<()> {
        if self.interest == EventSet::hup() {
            self.interest = EventSet::none();
            try!(event_loop.deregister(&self.sock));
            Ok(())
        } else {
            self.peer_hup = true;
            self.reregister(event_loop)
        }
    }


}

impl fmt::Display for Connection {
    fn fmt(&self, fmt : &mut fmt::Formatter) -> Result<(), fmt::Error> {
        try!(self.token.fmt(fmt));
        try!(<Display>::fmt(&", ", fmt));
        try!((&self.sock as *const TcpStream).fmt(fmt));
        try!(<Display>::fmt(&", ", fmt));
        self.sock.peer_addr().fmt(fmt)
    }
}

struct Server {
    sock: TcpListener,
    conns: Slab<Connection>,
}

impl Server {

    fn new(addr : SocketAddr) -> io::Result<(Server, EventLoop<Server>)> {

        let sock = try!(TcpListener::bind(&addr));

        let config = EventLoopConfig::new();
        let mut ev_loop : EventLoop<Server> = try!(EventLoop::configured(config));

        try!(ev_loop.register(&sock, SERVER, EventSet::readable(), PollOpt::edge()));

        Ok((Server {
            sock: sock,
            conns: Slab::new_starting_at(CONN_TOKEN_START, CONNS_MAX),
        }, ev_loop))
    }

    fn accept(&mut self, event_loop: &mut EventLoop<Server>) -> io::Result<()> {

        loop {
            let (sock, _addr) = match try!(self.sock.accept()) {
                None => break,
                Some(sock) => sock,
            };

            // Don't buffer output in TCP - kills latency sensitive benchmarks
            let _ = sock.set_nodelay(true);

            let conn = Connection::new(sock);

            let tok = self.conns.insert(conn);

            let tok = match tok {
                Ok(tok) => tok,
                Err(_) => return Ok(()),
            };

            self.conns[tok].token = Some(tok);

            try!(event_loop.register(
                    &self.conns[tok].sock, tok, EventSet::readable() , PollOpt::edge() | PollOpt::oneshot())
                );
        }

        Ok(())
    }

    fn conn_handle_finished(&mut self, tok : Token, finished : bool) {
        if finished {
            self.conns.remove(tok);
        }
    }

    fn conn_readable(&mut self, event_loop: &mut EventLoop<Server>, tok: Token) -> io::Result<()> {
        let (res, finished) = {
            let conn = self.conn(tok);
            let res = conn.readable(event_loop);
            (res, conn.is_finished())
        };
        self.conn_handle_finished(tok, finished);
        res
    }

    fn conn_writable(&mut self, event_loop: &mut EventLoop<Server>, tok: Token) -> io::Result<()> {
        let (res, finished) = {
            let conn = self.conn(tok);
            let res = conn.writable(event_loop);
            (res, conn.is_finished())
        };
        self.conn_handle_finished(tok, finished);
        res
    }

    fn conn_hup(&mut self, event_loop: &mut EventLoop<Server>, tok: Token) -> io::Result<()> {
        let (res, finished) = {
            let conn = self.conn(tok);
            let res = conn.hup(event_loop);
            (res, conn.is_finished())
        };
        self.conn_handle_finished(tok, finished);
        res
    }

    fn conn<'a>(&'a mut self, tok: Token) -> &'a mut Connection {
        &mut self.conns[tok]
    }
}

impl Handler for Server {
    type Timeout = usize;
    type Message = ();

    fn ready(&mut self, event_loop: &mut EventLoop<Server>, token: Token, events: EventSet) {

        let res = match token {
            SERVER => self.accept(event_loop),
            i => {
                if events.is_hup() {
                    let _ = self.conn_hup(event_loop, i);
                }
                if events.is_readable() {
                    let _ = self.conn_readable(event_loop, i);
                }
                if events.is_writable() {
                    let _ = self.conn_writable(event_loop, i);
                }

                Ok(())
            }
        };
        res.unwrap();
    }
}

pub fn main() {
    let addr = listend_addr();

    let (mut server, mut ev_loop) = Server::new(addr).unwrap();

    // Start the event loop
    println!("Starting mioecho server on {:?}", server.sock.local_addr().unwrap());
    ev_loop.run(&mut server).unwrap();
}
