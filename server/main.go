package main

import (
	"flag"
	"log"
	"net"
)

func main() {
	// Define tcp and udp address
	tcpAddr := flag.String("tcp", ":7700", "TCP control address")
	udpAddr := flag.String("udp", ":7701", "UDP media address")
	flag.Parse()

	// Starting listening to the tcp and udp buffers

	ltcp, err := net.Listen("tcp", *tcpAddr)
	if err != nil {
		log.Fatal(err)
	}
	defer ltcp.Close()

	ludp, err := net.ListenPacket("udp", *udpAddr)
	if err != nil {
		log.Fatal(err)
	}
	defer ludp.Close()

	// Server starting
	log.Printf("feather-chat server: control=%s media=%s", *tcpAddr, *udpAddr)
	serve(ltcp, ludp.(*net.UDPConn))
}

// serve runs the control and media loops until the listeners stop.
func serve(ltcp net.Listener, pc *net.UDPConn) {
	rm := NewRoomManager()
	go readUDP(pc, rm)
	for {
		c, err := ltcp.Accept()
		if err != nil {
			log.Println(err)
			return
		}
		go handleControl(c, rm)
	}
}
