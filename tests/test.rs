#[cfg(test)]
mod test {
    use fatcrab_trading::trader::Trader;

    #[tokio::test]
    async fn test_sanity() {
        let trader = Trader::new().await;
        let make_trade = trader.make_order();

        let (test_tx, mut test_rx) = tokio::sync::mpsc::channel::<String>(5);

        tokio::spawn(async move {
            let some_string = test_rx.recv().await.unwrap();
            println!("test_rx got some_string response {}", some_string);
        });

        make_trade.register_notif_tx(test_tx).await.unwrap();

        make_trade
            .register_notif_callback(|msg| {
                println!("notif callback with string: {}", msg);
            })
            .await
            .unwrap();
    }
}
