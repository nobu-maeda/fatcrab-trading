mod relay;

#[cfg(test)]
mod test {
    use std::net::SocketAddr;

    use uuid::Uuid;

    use fatcrab_trading::{
        maker::FatCrabMakerNotif,
        offer::FatCrabOffer,
        order::{FatCrabOrder, FatCrabOrderType},
        trader::FatCrabTrader,
    };

    use crate::relay::Relay;

    #[tokio::test]
    async fn test() {
        // Initial Relays
        let mut relays: Vec<Relay> = Vec::new();

        let relay: Relay = Relay::start();
        relay.wait_for_healthy_relay().await.unwrap();
        relays.push(relay);

        let relay: Relay = Relay::start();
        relay.wait_for_healthy_relay().await.unwrap();
        relays.push(relay);

        let relay: Relay = Relay::start();
        relay.wait_for_healthy_relay().await.unwrap();
        relays.push(relay);

        let relay: Relay = Relay::start();
        relay.wait_for_healthy_relay().await.unwrap();
        relays.push(relay);

        // Maker - Create Fatcrab Trader
        let trader_m = FatCrabTrader::new().await;

        // Taker - Create Fatcrab Trader
        let trader_t = FatCrabTrader::new().await;

        // Add Relays
        let mut relay_addrs: Vec<(String, Option<SocketAddr>)> = Vec::new();

        for relay in relays.iter_mut() {
            relay_addrs.push((format!("{}:{}", "ws://localhost", relay.port), None));
        }

        trader_m.add_relays(relay_addrs.clone()).await.unwrap();
        trader_t.add_relays(relay_addrs).await.unwrap();

        // Maker - Create Fatcrab Trade Order
        let trade_order = FatCrabOrder::Buy {
            trade_uuid: Uuid::new_v4(),
            amount: 100.0,
            price: 1000.0,
            fatcrab_acct_id: Uuid::new_v4(),
        };

        // Maker - Create Fatcrab Make Trader
        let maker = trader_m.make_order(trade_order).await;

        // Maker - Create channels & register Notif Tx
        let (tx, mut rx) = tokio::sync::mpsc::channel::<FatCrabMakerNotif>(5);
        maker.register_notif_tx(tx).await.unwrap();

        // Taker - Query Fatcrab Trade Order
        let orders = trader_t.query_orders(FatCrabOrderType::Sell).await.unwrap();
        assert_eq!(orders.len(), 0);

        let orders = trader_t.query_orders(FatCrabOrderType::Buy).await.unwrap();
        assert_eq!(orders.len(), 1);

        // Taker - Create Fatcrab Take Trader & Take Trade Order

        // Maker - Wait for Fatcrab Trader Order to be taken

        // Maker - Send Fatcrab Trade Response w/ Fatcrab address

        // Taker - Wait for Fatcrab Trade Response

        // Taker - Signal user to remit Fatcrab

        // Taker - User claims Fatcrab remittance, send Peer Message with Bitcoin address

        // Maker - Wait for Fatcrab Peer Message

        // Maker - Send Bitcoin to Taker Bitcoin address

        // Maker - Send Fatcrab Peer Message with Bitcoin txid

        // Maker - Trade Completion

        // Taker - Wait for Fatcrab Peer Message

        // Taker - Confirm Bitcoin txid

        // Taker - Trade Completion
    }
}
