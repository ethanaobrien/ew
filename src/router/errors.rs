use actix_web::{HttpRequest, HttpResponse};
use crate::router::global;

#[allow(dead_code)]
#[derive(Clone, Copy)]
#[repr(i32)]
pub enum ResultCode {
    Success = 0,
    ErrorGeneric = 1,
    ErrorInternalServerError = 2,
    ErrorBadRequest = 3,
    ErrorNotFound = 4,
    ErrorUnauthorized = 5,
    ErrorAdminUnauthorized = 6,
    ErrorForbidden = 7,
    ErrorNotImplemented = 8,
    ErrorServiceUnavailable = 9,
    ErrorMaintenance = 10,
    ErrorDataMismatch = 11,
    ErrorGameVersionUpdated = 12,
    ErrorResourceUpdated = 13,
    ErrorSession = 14,
    ErrorFunctionLock = 15,
    ErrorUserNotFound = 16,
    ErrorUserStaminaMax = 17,
    ErrorUserStaminaShortage = 18,
    ErrorItemNotFound = 19,
    ErrorItemShortage = 20,
    ErrorItemUnknownEffect = 21,
    ErrorItemExpired = 22,
    ErrorPointShortage = 23,
    ErrorFriendMaxSelf = 24,
    ErrorFriendMaxOther = 25,
    ErrorFriendRequestMaxSelf = 26,
    ErrorFriendRequestMaxOther = 27,
    ErrorFriendPendingMaxSelf = 28,
    ErrorFriendPendingMaxOther = 29,
    ErrorFriendAlreadyRequest = 30,
    ErrorFriendAlreadyPending = 31,
    ErrorFriendAlreadyFriend = 32,
    ErrorFriendRequestReject = 33,
    ErrorFriendNotFound = 34,
    ErrorEventClosed = 35,
    ErrorMissionExpired = 36,
    ErrorGiftExpired = 37,
    ErrorMigrateWrongParam = 38,
    ErrorMigrateAlreadyCompleted = 39,
    ErrorMultiPenaltyPeriod = 40,
    ErrorMultiMissRatio = 41,
    ErrorMultiLiveLevelNotAvailable = 42,
    WarningException = 10000,
}

impl std::fmt::Display for ResultCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ResultCode::Success => "SUCCESS",
            ResultCode::ErrorGeneric => "ERROR_GENERIC",
            ResultCode::ErrorInternalServerError => "ERROR_INTERNAL_SERVER",
            ResultCode::ErrorBadRequest => "ERROR_BAD_REQUEST",
            ResultCode::ErrorNotFound => "ERROR_NOT_FOUND",
            ResultCode::ErrorUnauthorized => "ERROR_UNAUTHORIZED",
            ResultCode::ErrorAdminUnauthorized => "ERROR_ADMIN_UNAUTHORIZED",
            ResultCode::ErrorForbidden => "ERROR_FORBIDDEN",
            ResultCode::ErrorNotImplemented => "ERROR_NOT_IMPLEMENTED",
            ResultCode::ErrorServiceUnavailable => "ERROR_SERVICE_UNAVAILABLE",
            ResultCode::ErrorMaintenance => "ERROR_MAINTENANCE",
            ResultCode::ErrorDataMismatch => "ERROR_DATA_MISMATCH",
            ResultCode::ErrorGameVersionUpdated => "ERROR_GAME_VERSION_UPDATED",
            ResultCode::ErrorResourceUpdated => "ERROR_RESOURCE_UPDATED",
            ResultCode::ErrorSession => "ERROR_SESSION",
            ResultCode::ErrorFunctionLock => "ERROR_FUNCTION_LOCK",
            ResultCode::ErrorUserNotFound => "ERROR_USER_NOT_FOUND",
            ResultCode::ErrorUserStaminaMax => "ERROR_USER_STAMINA_MAX",
            ResultCode::ErrorUserStaminaShortage => "ERROR_USER_STAMINA_SHORTAGE",
            ResultCode::ErrorItemNotFound => "ERROR_ITEM_NOT_FOUND",
            ResultCode::ErrorItemShortage => "ERROR_ITEM_SHORTAGE",
            ResultCode::ErrorItemUnknownEffect => "ERROR_ITEM_UNKNOWN_EFFECT",
            ResultCode::ErrorItemExpired => "ERROR_ITEM_EXPIRED",
            ResultCode::ErrorPointShortage => "ERROR_POINT_SHORTAGE",
            ResultCode::ErrorFriendMaxSelf => "ERROR_FRIEND_MAX_SELF",
            ResultCode::ErrorFriendMaxOther => "ERROR_FRIEND_MAX_OTHER",
            ResultCode::ErrorFriendRequestMaxSelf => "ERROR_FRIEND_REQUEST_MAX_SELF",
            ResultCode::ErrorFriendRequestMaxOther => "ERROR_FRIEND_REQUEST_MAX_OTHER",
            ResultCode::ErrorFriendPendingMaxSelf => "ERROR_FRIEND_PENDING_MAX_SELF",
            ResultCode::ErrorFriendPendingMaxOther => "ERROR_FRIEND_PENDING_MAX_OTHER",
            ResultCode::ErrorFriendAlreadyRequest => "ERROR_FRIEND_ALREADY_REQUEST",
            ResultCode::ErrorFriendAlreadyPending => "ERROR_FRIEND_ALREADY_PENDING",
            ResultCode::ErrorFriendAlreadyFriend => "ERROR_FRIEND_ALREADY_FRIEND",
            ResultCode::ErrorFriendRequestReject => "ERROR_FRIEND_REQUEST_REJECT",
            ResultCode::ErrorFriendNotFound => "ERROR_FRIEND_NOT_FOUND",
            ResultCode::ErrorEventClosed => "ERROR_EVENT_CLOSED",
            ResultCode::ErrorMissionExpired => "ERROR_MISSION_EXPIRED",
            ResultCode::ErrorGiftExpired => "ERROR_GIFT_EXPIRED",
            ResultCode::ErrorMigrateWrongParam => "ERROR_MIGRATE_WRONG_PARAM",
            ResultCode::ErrorMigrateAlreadyCompleted => "ERROR_MIGRATE_ALREADY_COMPLETED",
            ResultCode::ErrorMultiPenaltyPeriod => "ERROR_MULTI_PENALTY_PERIOD",
            ResultCode::ErrorMultiMissRatio => "ERROR_MULTI_MISS_RATIO",
            ResultCode::ErrorMultiLiveLevelNotAvailable => "ERROR_MULTI_LIVE_LEVEL_NOT_AVAILABLE",
            ResultCode::WarningException => "WARNING_EXCEPTION",
        };
        write!(f, "{}", s)
    }
}

pub struct ErrorResponse(pub HttpRequest, pub ResultCode);

impl std::fmt::Debug for ErrorResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} for uid {}", self.1, global::get_uid(self.0.headers()))
    }
}

impl std::fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl actix_web::ResponseError for ErrorResponse {
    fn error_response(&self) -> HttpResponse {
        println!("Rejecting request from uid {}: {}", global::get_uid(self.0.headers()), self.1);
        global::api_error(&self.0, self.1)
    }
}
