use super::responses::*;
use crate::{
    domain::offer::{message::GetOpenOffers, OfferBook},
    prelude::{Addr, Future},
};
use actix_web::{
    web::{self, Data},
    App, Error, HttpServer, Result,
};
use std::io;

pub fn listen(port: u16, offer_book: Addr<OfferBook>) -> Result<(), io::Error> {
    let data = web::Data::new(offer_book);
    HttpServer::new(move || {
        App::new()
            .register_data(data.clone())
            .route("/ping", web::get().to(|| "pong"))
            .route("/offers", web::get().to_async(get_offers))
    })
    .workers(1)
    .bind(("127.0.0.1", port))?
    .start();
    Ok(())
}

fn get_offers(
    data: Data<Addr<OfferBook>>,
) -> impl Future<Item = web::Json<GetOffers>, Error = Error> {
    data.get_ref()
        .send(GetOpenOffers)
        .map(|offers| web::Json(GetOffers::from(offers)))
        .map_err(|e| e.into())
}
