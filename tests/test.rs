#[cfg(test)]
mod test {
    use fatcrab_trading::{trade_order::TradeOrder, trader::Trader};
    use uuid::Uuid;

    #[tokio::test]
    async fn test_sanity() {
        let trader = Trader::new().await;
        let trade_order = TradeOrder::Buy {
            amount: 100,
            price: 1000.0,
            fatcrab_acct_id: Uuid::new_v4(),
        };
        let make_trade = trader.make_order(trade_order).await;

        let (test_tx, mut test_rx) = tokio::sync::mpsc::channel::<String>(5);

        tokio::spawn(async move {
            let some_string = test_rx.recv().await.unwrap();
            println!("test_rx got some_string response {}", some_string);
        });

        make_trade.register_notif_tx(test_tx).await.unwrap();
    }
}
