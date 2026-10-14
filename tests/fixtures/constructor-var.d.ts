// A constructor-shaped variable with no interface of the same name is
// still a class. The construct signature's return type describes the
// instance: an inline literal supplies the members, a named type becomes
// the parent.

interface WebSocket {
  send(data: string): void;
}

declare const WebSocketPair: {
  new (): {
    0: WebSocket;
    1: WebSocket;
  };
};

interface Socket {
  readonly readable: boolean;
}

declare const TcpSocket: {
  new (address: string): Socket;
  readonly defaultPort: number;
};

// A `prototype`-only literal (no construct signature) is a class too,
// without a constructor.
declare const Registry: {
  prototype: Socket;
  readonly size: number;
};
