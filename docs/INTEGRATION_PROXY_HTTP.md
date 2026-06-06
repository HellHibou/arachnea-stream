Ajouter la gestion des proxys pour chaque moteur HTTP. Préférer les connecteurs directs `arachnea-proxy` lorsqu'ils sont disponibles, tout en permettant toujours une configuration explicite de proxy réseau.

# ghostwire
La stratégie : Le connecteur tokio::io::duplex

Au lieu de passer une URL textuelle comme "http://127.0.0.1:1080" à Ghostwire, vous allez interconnecter virtuellement les deux composants via un câble réseau virtuel en mémoire.
- tokio::io::duplex(max_buf_size) crée une paire de sockets virtuels connectés l'un à l'autre (client_side et server_side). Tout ce qui est écrit dans l'un est instantanément lu dans l'autre. 
- Vous injectez le côté client_side dans le connecteur personnalisé de Ghostwire (généralement via le client reqwest ou hyper sous-jacent qu'il expose dans son Builder).
- Votre code de proxy maison tourne en tâche de fond (tokio::spawn) et écoute directement sur le server_side en mémoire, sans aucun socket réseau physique.

Voici comment structurer ce montage dans votre application :

```
use tokio::io::{duplex, DuplexStream};
use ghostwire::Ghostwire; // Selon l'organisation exacte de leur API client

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. On crée le tunnel "virtuel" en mémoire (sans aucun port réseau !)
    // client_io ira dans Ghostwire, proxy_io ira dans votre logique maison
    let (client_io, proxy_io) = duplex(64 * 1024);

    // 2. On lance votre proxy maison en arrière-plan
    // Il va lire et écrire directement sur le flux proxy_io
    tokio::spawn(async move {
        if let Err(e) = mon_proxy_maison_in_memory(proxy_io).await {
            eprintln!("Erreur dans le proxy maison: {:?}", e);
        }
    });

    // 3. On configure Ghostwire pour injecter le socket virtuel
    // Note : Selon la version de Ghostwire, cela se fait en configurant 
    // le connecteur personnalisé (connector) du client HTTP interne.
    let scraper = Ghostwire::builder()
        .custom_connector(move |_url| {
            // Au lieu d'ouvrir une connexion TCP vers internet, 
            // Ghostwire croit se connecter à un proxy mais écrit dans notre canal
            let client_io_clone = client_io.clone(); 
            async move { Ok(client_io_clone) }
        })
        .build()?;

    // 4. Utilisation classique : Ghostwire va contourner Cloudflare en transitant 
    // de manière invisible par votre code custom avant de sortir sur internet.
    let resp = scraper.get("https://site-protege.com").await?;
    println!("Statut : {}", resp.status());

    Ok(())
}

// Votre logique de proxy custom qui traite les données directement en mémoire
async fn mon_proxy_maison_in_memory(mut stream: DuplexStream) -> anyhow::Result<()> {
    println!("Le proxy custom a intercepté une requête de Ghostwire en mémoire !");
    
    // Insérez votre code ici :
    // 1. Lire les octets (la requête HTTP/TLS envoyée par Ghostwire)
    // 2. Modifier les paquets, appliquer votre routage propriétaire, etc.
    // 3. Envoyer le résultat vers la vraie machine distante (via une vraie socket cette fois)
    // 4. Répondre à Ghostwire en écrivant dans `stream`
    
    Ok(())
}
```

Les avantages de cette approche :
- Zéro port ouvert : Aucun outil tiers ou processus sur la machine ne peut intercepter ou voir le trafic entre Ghostwire et votre proxy.
- Vitesse maximale : L'échange se fait par copie de pointeurs en mémoire RAM, évitant la latence de la pile réseau de votre système d'exploitation (Loopback TCP stack).
- Isolation : Vous pouvez lancer 50 instances de votre programme en même temps sans jamais avoir d'erreur de type Address already in use (port coincé).

Pour valider l'intégration exacte, savez-vous si votre version de Ghostwire permet d'accéder à la méthode .connector() ou si le builder n'autorise que les chaînes de caractères de type Proxy ? Dans le second cas, il faudra ruser en modifiant légèrement l'initialisation du client de transport.


# rquest

Comme rquest rquest - crates.io est un fork de reqwest rquest - crates.io (conçu spécifiquement pour l'émulation TLS/JA3/JA4 et l'usurpation d'identité de navigateurs avec BoringSSL rquest - crates.io), son architecture de build conserve le même concept de Connector Layer reqwest - Docs.rs.

Le fait que rquest utilise BoringSSL en interne ne change rien à la gestion des flux asynchrones : vous pouvez injecter un câble réseau virtuel en mémoire (tokio::io::duplex) DuplexStream in tokio::io - Rust - Docs.rs directement au moment de construire le client rquest.

Voici le code exact pour lier rquest et votre logique de proxy maison 100 % en mémoire vive, sans ouvrir de port DuplexStream in tokio::io - Rust - Docs.rs :

```
use std::error::Error;
use rquest::tls::Impersonate;
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt, DuplexStream};
use tower::util::service_fn;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1. Création des deux extrémités du canal réseau virtuel en mémoire RAM
    // client_io -> va être donné à rquest
    // proxy_io  -> va être consommé par votre proxy maison
    let (client_io, proxy_io) = duplex(64 * 1024);

    // 2. Lancement en arrière-plan de votre logique de proxy maison
    tokio::spawn(async move {
        if let Err(e) = mon_proxy_maison(proxy_io).await {
            eprintln!("[Proxy] Erreur : {:?}", e);
        }
    });

    // 3. Configuration du client `rquest` avec usurpation de navigateur
    // (Exemple : On imite Chrome 128 tout en redirigeant le flux réseau en mémoire)
    let client = rquest::Client::builder()
        .impersonate(Impersonate::Chrome128) // Signature TLS/JA4 Cloudflare-bypass
        .enable_ech_grease()
        .permute_extensions()
        .connector_layer(service_fn(move |_target| {
            // À chaque fois que rquest initie une connexion, il passe par ici.
            // On lui donne l'accès à notre socket virtuel en mémoire
            let stream_clone = client_io.clone();
            async move { Ok::<_, std::io::Error>(stream_clone) }
        }))
        .build()?;

    // 4. Exécution de la requête : les empreintes TLS et le trafic
    // vont être envoyés directement dans votre fonction proxy en RAM.
    println!("[Main] Envoi de la requête camouflée...");
    let resp = client.get("https://tls.peet.ws/api/all").send().await?;

    println!("[Main] Réponse reçue du proxy ! Statut : {}", resp.status());
    Ok(())
}

// --- VOTRE LOGIQUE DE PROXY CUSTOM ---
async fn mon_proxy_maison(mut stream: DuplexStream) -> Result<(), Box<dyn Error + Send + Sync>> {
    println!("[Proxy] Connexion reçue en mémoire (aucun port ouvert !)");

    // Étape A : Lire le flux asynchrone (Requête HTTP brute ou poignée de main TLS)
    let mut buffer = vec![0; 4096];
    let n = stream.read(&mut buffer).await?;
    
    // Le tableau `buffer[..n]` contient les données envoyées par rquest.
    // NOTE : Comme rquest fait du HTTPS (TLS/JA4), les données ici seront chiffrées 
    // par BoringSSL si l'URL est en "https://". C'est un flux binaire brut (ClientHello TLS).
    println!("[Proxy] Lu {} octets depuis rquest en mémoire.", n);

    // Étape B : Votre code custom ici
    // - Vous pouvez inspecter/modifier le flux binaire
    // - Vous devez ouvrir un vrai TcpStream (ou autre canal) vers la cible finale
    // - Relayer les octets du buffer vers la cible, et vice-versa.

    // Étape C : Exemple de simulation de réponse
    // (Si rquest s'attend à du HTTP classique en texte brut "http://")
    let fake_http_response = "HTTP/1.1 200 OK\r\nContent-Length: 21\r\n\r\nHello depuis le proxy";
    stream.write_all(fake_http_response.as_bytes()).await?;
    stream.flush().await?;

    Ok(())
}
```

Point de vigilance technique propre à rquest :
Puisque le but de rquest est de bypasser Cloudflare en chiffrant le trafic avec des empreintes TLS complexes (Impersonate::Chrome128) [GitHub - 0x676e67/rquest-deprecated], le flux d'octets que votre fonction mon_proxy_maison va intercepter en premier sera le handshake TLS (les paquets binaires chiffrés) et non du texte HTTP brut (sauf si vous ciblez une adresse http://).

Votre proxy maison doit donc être conçu comme un Forwarder de flux TCP (Stream Relayer) : son rôle sera de prendre ces octets binaires cryptés, de les envoyer vers la machine distante sur Internet via un vrai socket sortant, puis de renvoyer la réponse binaire distante dans le DuplexStream DuplexStream in tokio::io - Rust - Docs.rs. De cette façon, rquest gère le chiffrement de bout en bout et Cloudflare ne voit que du feu, tandis que vous maîtrisez totalement la couche transport sans ouvrir de port local.
