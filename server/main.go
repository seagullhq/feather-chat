package main

import (
	"context"
	"flag"
	"fmt"
	"log"
	"net"
	"net/url"
	"os"
	"time"

	"github.com/joho/godotenv"
	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/migrations"
	"go.mongodb.org/mongo-driver/v2/mongo"
	"go.mongodb.org/mongo-driver/v2/mongo/options"
)

func main() {
	if err := godotenv.Load(); err != nil {
		if err2 := godotenv.Load("../.env"); err2 != nil {
			log.Printf(".env not loaded (%v); using environment only", err)
		}
	}

	mongoURI := flag.String("mongo", defaultMongoURI(), "MongoDB connection URI")
	dbName := flag.String("db", os.Getenv("MONGO_DBNAME"), "MongoDB database name")

	// Define tcp and udp address
	tcpAddr := flag.String("tcp", ":7700", "TCP control address")
	udpAddr := flag.String("udp", ":7701", "UDP media address")
	flag.Parse()

	// Connect to MongoDB and run the migrations before accepting clients
	db, err := mongo.Connect(options.Client().ApplyURI(*mongoURI))
	if err != nil {
		log.Fatal(err)
	}
	defer db.Disconnect(context.Background())

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	if err := db.Ping(ctx, nil); err != nil {
		log.Fatalf("mongo ping: %v", err)
	}
	if err := migrations.Run(ctx, db.Database(*dbName)); err != nil {
		log.Fatalf("migrations: %v", err)
	}

	store := accounts.NewStore(db.Database(*dbName))

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
	serve(listenerTCP, listenerUDP.(*net.UDPConn), store)
}

func defaultMongoURI() string {
	return fmt.Sprintf(
		"mongodb://%s:%s@%s:27017/?authSource=admin&directConnection=true",
		url.QueryEscape(os.Getenv("MONGO_ROOT_USERNAME")),
		url.QueryEscape(os.Getenv("MONGO_ROOT_PASSWORD")),
		url.QueryEscape(os.Getenv("MONGO_HOST")),
	)
}

// serve runs the control and media loops until the listeners stop.
func serve(ltcp net.Listener, pc *net.UDPConn, users *accounts.Store) {
	roomManager := NewRoomManager()
	go readUDP(pc, roomManager)
	for {
		conn, err := ltcp.Accept()
		if err != nil {
			log.Println(err)
			return
		}
		go handleControl(conn, roomManager, users)
	}
}
