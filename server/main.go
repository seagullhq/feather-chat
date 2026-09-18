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

	listenerTCP, err := net.Listen("tcp", *tcpAddr)
	if err != nil {
		log.Fatal(err)
	}
	defer listenerTCP.Close()

	listenerUDP, err := net.ListenPacket("udp", *udpAddr)
	if err != nil {
		log.Fatal(err)
	}
	defer listenerUDP.Close()

	// Server starting
	log.Printf("feather-chat server: control=%s media=%s", *tcpAddr, *udpAddr)
	serve(listenerTCP, listenerUDP.(*net.UDPConn))
}

// serve runs the control and media loops until the listeners stop.
func serve(ltcp net.Listener, pc *net.UDPConn) {
	roomManagaer := NewRoomManager()
	go readUDP(pc, roomManagaer)
	for {
		conn, err := ltcp.Accept()
		if err != nil {
			log.Println(err)
			return
		}
		go handleControl(conn, roomManagaer)
	}
}
