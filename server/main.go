package main

import (
	"context"
	"database/sql"
	"flag"
	"fmt"
	"log"
	"net"
	"net/url"
	"os"
	"time"

	"github.com/joho/godotenv"
	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/servers"

	// Driver PostgreSQL tramite pgx compatibile con database/sql
	_ "github.com/jackc/pgx/v5/stdlib"

	// Librerie per le migrazioni
	"github.com/golang-migrate/migrate/v4"
	"github.com/golang-migrate/migrate/v4/database/postgres"
	"github.com/golang-migrate/migrate/v4/source/iofs"
)

func main() {
	if err := godotenv.Load(); err != nil {
		if err2 := godotenv.Load("../.env"); err2 != nil {
			log.Printf(".env not loaded (%v); using environment only", err)
		}
	}

	pgURI := flag.String("postgres", defaultPostgresURI(), "PostgreSQL connection URI")

	// Define tcp and udp address
	tcpAddr := flag.String("tcp", ":7700", "TCP control address")
	udpAddr := flag.String("udp", ":7701", "UDP media address")
	flag.Parse()

	// 1. Connessione a PostgreSQL usando il driver pgx
	db, err := sql.Open("pgx", *pgURI)
	if err != nil {
		log.Fatalf("Errore apertura connessione DB: %v", err)
	}
	defer db.Close()

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	// Verifica la connessione
	if err := db.PingContext(ctx); err != nil {
		log.Fatalf("postgres ping fallito: %v", err)
	}
	log.Println("Connesso a PostgreSQL con successo")

	// 2. Esegui le migrazioni
	runMigrations(db)

	// 3. Inizializza gli store passandogli l'istanza *sql.DB
	store := accounts.NewStore(db)
	serverStore := servers.NewStore(db)

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
	serve(listenerTCP, listenerUDP.(*net.UDPConn), store, serverStore)
}

func defaultPostgresURI() string {
	return fmt.Sprintf(
		"postgres://%s:%s@%s:5432/%s",
		url.QueryEscape(os.Getenv("POSTGRES_USER")),
		url.QueryEscape(os.Getenv("POSTGRES_PASSWORD")),
		os.Getenv("POSTGRES_HOST"),
		os.Getenv("POSTGRES_DB"),
	)
}

// runMigrations applica automaticamente tutte le migrazioni non ancora eseguite all'avvio
func runMigrations(db *sql.DB) {
	driver, err := postgres.WithInstance(db, &postgres.Config{})
	if err != nil {
		log.Fatalf("Errore configurazione driver migrazioni: %v", err)
	}

	// Le migrazioni sono embedded nel binario: le trova da qualsiasi directory.
	src, err := iofs.New(migrationFS, "internal/migrations/sql")
	if err != nil {
		log.Fatalf("Errore inizializzazione sorgente migrazioni: %v", err)
	}

	m, err := migrate.NewWithInstance("iofs", src, "postgres", driver)
	if err != nil {
		log.Fatalf("Errore inizializzazione migrazioni: %v", err)
	}

	if err := m.Up(); err != nil && err != migrate.ErrNoChange {
		log.Fatalf("Errore esecuzione migrazioni: %v", err)
	}

	if err == migrate.ErrNoChange {
		log.Println("Nessuna nuova migrazione da applicare.")
	} else {
		log.Println("Migrazioni applicate con successo!")
	}
}

// serve runs the control and media loops until the listeners stop.
func serve(ltcp net.Listener, pc *net.UDPConn, users *accounts.Store, serverStore *servers.ServerStore) {
	roomManager := NewRoomManager()
	go readUDP(pc, roomManager)
	for {
		conn, err := ltcp.Accept()
		if err != nil {
			log.Println(err)
			return
		}
		go handleControl(conn, roomManager, users, serverStore)
	}
}
