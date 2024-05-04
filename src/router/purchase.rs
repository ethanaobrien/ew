use json::object;
use actix_web::{HttpResponse, HttpRequest};

use crate::router::global;

pub fn purchase(_req: HttpRequest) -> HttpResponse {
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "product_list": [//Client will error if this is an empty array
                {
                    "product_id": "com.bushiroad.global.lovelive.sif2.google.promo.4199",
                    "name": "6000 Love Gems",
                    "description": "6000 Paid Love Gems",
                    "thumbnail_url": null,
                    "charge_gem": 6000,
                    "free_gem": 0,
                    "campaign_type": 2,
                    "campaign_mode": 5,
                    "priority": 1,
                    "price": "999999.99",
                    "currency_code": "USD",
                    "formatted_price": "USD$999999.99",
                    "consumable": 0,
                    "limited_count": 2,
                    "product_type": 0,
                    "amenity_label": null,
                    "ticket_valid_days": null,
                    "ticket_issuing_gem": null,
                    "start_datetime": "2024-02-01 00:00:00",
                    "end_datetime": "2024-02-29 23:59:59",
                    "total_gem": 6000
                }
            ]
        }
    };
    global::send(resp)
}
