extern crate mio;
#[macro_use]
extern crate log;
extern crate env_logger;


use mio::*;
use mio::tcp::*;
use mio::buf::{RingBuf};
use mio::util::Slab;
use std::io;
use std::fmt;
use std::fmt::Debug;
use std::fmt::Display;

use std::net::SocketAddr;
use std::str::FromStr;

pub fn localhost() -> SocketAddr {
    let s = "127.0.0.1:18080";
    FromStr::from_str(&s).unwrap()
}

pub fn sleep_ms(ms: usize) {
    use std::thread;
    thread::sleep_ms(ms as u32);
}

const SERVER: Token = Token(0);
const CONN_TOKEN_START : Token = Token(1);

enum ConnResult {
    Io(io::Result<()>),
    Disconnect,
}

struct EchoConn {
    sock: TcpStream,
    buf: RingBuf,
    token: Option<Token>,
    peer_hup: bool,
}

impl EchoConn {
    fn new(sock: TcpStream) -> EchoConn {
        EchoConn {
            sock: sock,
            buf: RingBuf::new(1024),
            token: None,
            peer_hup: false,
        }
    }

    fn reregister(&mut self,
                  event_loop: &mut EventLoop<Echo>,
                 ) -> ConnResult {

        let mut interest = Interest::hup();

        // have somewhere to write
        if self.buf.bytes().len() > 0 {
            interest.insert(Interest::writable());
        }

        // have somewhere to read to
        if !self.peer_hup && self.buf.mut_bytes().len() > 0 {
            interest.insert(Interest::readable());
        }

        if self.peer_hup && self.buf.bytes().len() == 0 {
            event_loop.deregister(&self.sock).unwrap();
            debug!("{} deregister", self);
            ConnResult::Disconnect
        } else {
            ConnResult::Io(event_loop.reregister(
                &self.sock, self.token.unwrap(),
                interest, PollOpt::edge() | PollOpt::oneshot()
                ))
        }
    }

    fn hup(&mut self,
           event_loop: &mut EventLoop<Echo>,
          ) -> ConnResult {
        self.peer_hup = true;
        self.reregister(event_loop)
    }

    fn writable(&mut self,
                event_loop: &mut EventLoop<Echo>,
                ) -> ConnResult {

        match self.sock.write_slice(self.buf.bytes()) {
            Ok(None) => {
                debug!("{}: wouldblock", self);
            }
            Ok(Some(r)) => {
                debug!("{}: wrote {} bytes!", self, r);
                Buf::advance(&mut self.buf, r);
            }
            Err(e) => debug!("not implemented; client err={:?}", e),
        }

        self.reregister(event_loop)
    }

    fn readable(&mut self,
                event_loop: &mut EventLoop<Echo>,
                ) -> ConnResult {

/*        if self.peer_hup {
            info!("PROBLEM: peer_hup already set");
        }*/

        match self.sock.read_slice(&mut self.buf.mut_bytes()) {
            Ok(None) => {
                panic!("We just got readable, but were unable to read from the socket?");
            }
            Ok(Some(r)) => {
                debug!("{}: read {} bytes!", self, r);
                MutBuf::advance(&mut self.buf, r);
            }
            Err(e) => {
                debug!("not implemented; client err={:?}", e);
            }

        };

        self.reregister(event_loop)
    }
}

impl fmt::Display for EchoConn {
    fn fmt(&self, fmt : &mut fmt::Formatter) -> Result<(), fmt::Error> {
        try!(self.token.fmt(fmt));
        try!(<Display>::fmt(&", ", fmt));
        try!((&self.sock as *const TcpStream).fmt(fmt));
        try!(<Display>::fmt(&", ", fmt));
        self.sock.peer_addr().fmt(fmt)
    }
}

struct EchoServer {
    sock: TcpListener,
    conns: Slab<EchoConn>
}

impl EchoServer {
    fn accept(&mut self, event_loop: &mut EventLoop<Echo>) -> io::Result<()> {

        let sock = self.sock.accept().unwrap().unwrap();
        let conn = EchoConn::new(sock);
        let tok = self.conns.insert(conn)
            .ok().expect("could not add connection to slab");
        debug!("{:?} - new token", tok);

        self.conns[tok].token = Some(tok);

        debug!("{}: new connection", self.conns[tok]);
        event_loop.register_opt(&self.conns[tok].sock, tok, Interest::readable() , PollOpt::edge() | PollOpt::oneshot())
            .ok().expect("could not register socket with event loop");

        Ok(())
    }

    fn conn_handle_result(&mut self, tok : Token, res : ConnResult) -> io::Result<()> {
        match res {
            ConnResult::Disconnect => {
                debug!("Dropping {}", self.conn(tok));
                self.conns.remove(tok);
                Ok(())
            },
            ConnResult::Io(io) => {io}
        }
    }

    fn conn_readable(&mut self, event_loop: &mut EventLoop<Echo>, tok: Token) -> io::Result<()> {
        let res = self.conn(tok).readable(event_loop);
        self.conn_handle_result(tok, res)
    }

    fn conn_writable(&mut self, event_loop: &mut EventLoop<Echo>, tok: Token) -> io::Result<()> {
        let res = self.conn(tok).writable(event_loop);
        self.conn_handle_result(tok, res)
    }

    fn conn_hup(&mut self, event_loop: &mut EventLoop<Echo>, tok: Token) -> io::Result<()> {
        let res = self.conn(tok).hup(event_loop);
        self.conn_handle_result(tok, res)
    }

    fn conn<'a>(&'a mut self, tok: Token) -> &'a mut EchoConn {
        &mut self.conns[tok]
    }
}


struct Echo {
    server: EchoServer,
}

impl Echo {
    fn new(srv: TcpListener) -> Echo {
        Echo {
            server: EchoServer {
                sock: srv,
                conns: Slab::new_starting_at(CONN_TOKEN_START, 32)
            },
        }
    }
}

impl Handler for Echo {
    type Timeout = usize;
    type Message = ();

    fn readable(&mut self, event_loop: &mut EventLoop<Echo>, token: Token, hint: ReadHint) {

        match token {
            SERVER => self.server.accept(event_loop).unwrap(),
            i => {
                if hint.is_hup() {
                    self.server.conn_hup(event_loop, i).unwrap();
                } else {
                    self.server.conn_readable(event_loop, i).unwrap()
                }
            }
        };
    }

    fn writable(&mut self, event_loop: &mut EventLoop<Echo>, token: Token) {
        match token {
            SERVER => panic!("received writable for token 0"),
            _ => self.server.conn_writable(event_loop, token).unwrap()
        };
    }
}

pub fn main() {
    env_logger::init().unwrap();

    info!("Starting mioecho server");
    let mut event_loop = EventLoop::new().unwrap();

    let addr = localhost();
    let srv = TcpSocket::v4().unwrap();

    srv.set_reuseaddr(true).unwrap();
    srv.bind(&addr).unwrap();

    let srv = srv.listen(256).unwrap();

    info!("listen for connections on {:?}", addr);
    event_loop.register_opt(&srv, SERVER, Interest::readable(), PollOpt::edge()).unwrap();

    // Start the event loop
    event_loop.run(&mut Echo::new(srv)).unwrap();
}
