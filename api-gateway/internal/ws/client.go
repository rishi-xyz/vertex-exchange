package ws

import (
	"time"

	"github.com/gorilla/websocket"
)

const (
	writeWait  = 10 * time.Second
	pongWait   = 60 * time.Second
	pingPeriod = 30 * time.Second
	maxMsgSize = 4096
)

// Client is a single websocket connection subscribed to one pair.
type Client struct {
	hub  *Hub
	conn *websocket.Conn
	send chan []byte
	pair string
}

// Serve registers the connection and runs its read and write loops until the
// peer disconnects.
func (h *Hub) Serve(conn *websocket.Conn, pair string) {
	conn.SetReadLimit(maxMsgSize)
	conn.SetReadDeadline(time.Now().Add(pongWait))
	conn.SetPongHandler(func(string) error {
		conn.SetReadDeadline(time.Now().Add(pongWait))
		return nil
	})

	c := &Client{
		hub:  h,
		conn: conn,
		send: make(chan []byte, 32),
		pair: pair,
	}
	if pair != "" {
		h.Subscribe(pair, c)
	}

	writeDone := make(chan struct{})
	go c.writeLoop(writeDone)

	c.readLoop()

	h.UnsubscribeAll(c)
	<-writeDone
}

func (c *Client) readLoop() {
	defer c.conn.Close()
	for {
		if _, _, err := c.conn.ReadMessage(); err != nil {
			return
		}
	}
}

func (c *Client) writeLoop(done chan struct{}) {
	defer close(done)
	ticker := time.NewTicker(pingPeriod)
	defer ticker.Stop()
	for {
		select {
		case msg, ok := <-c.send:
			c.conn.SetWriteDeadline(time.Now().Add(writeWait))
			if !ok {
				c.conn.WriteMessage(websocket.CloseMessage, []byte{})
				return
			}
			if err := c.conn.WriteMessage(websocket.TextMessage, msg); err != nil {
				return
			}
		case <-ticker.C:
			c.conn.SetWriteDeadline(time.Now().Add(writeWait))
			if err := c.conn.WriteMessage(websocket.PingMessage, nil); err != nil {
				return
			}
		}
	}
}
