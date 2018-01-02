use bytes::Bytes;
use context::{Context, Filter, RhsValue, Type};

use cidr::{Cidr, IpCidr};
use regex::Regex;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::net::IpAddr;

#[derive(Default)]
pub struct ExecutionContext(HashMap<String, LhsValue>);

impl ExecutionContext {
    pub fn new(map: HashMap<String, LhsValue>) -> Self {
        ExecutionContext(map)
    }
}

nested_enum!(#[derive(Debug)] LhsValue {
    IpAddr(IpAddr),
    Bytes(Bytes),
    Unsigned(u64),
});

fn range_order<T: Ord>(lhs: T, rhs_first: T, rhs_last: T) -> Ordering {
    match (lhs.cmp(&rhs_first), lhs.cmp(&rhs_last)) {
        (Ordering::Less, _) => Ordering::Less,
        (_, Ordering::Greater) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

fn ip_order<T>(lhs: &T::Address, rhs: &T) -> Ordering
where
    T: Cidr,
    T::Address: Ord,
{
    range_order(lhs, &rhs.first_address(), &rhs.last_address())
}

impl PartialEq<RhsValue> for LhsValue {
    fn eq(&self, other: &RhsValue) -> bool {
        self.partial_cmp(other) == Some(Ordering::Equal)
    }
}

impl PartialOrd<RhsValue> for LhsValue {
    fn partial_cmp(&self, other: &RhsValue) -> Option<Ordering> {
        Some(match (self, other) {
            (
                &LhsValue::IpAddr(IpAddr::V4(ref addr)),
                &RhsValue::IpCidr(IpCidr::V4(ref network)),
            ) => ip_order(addr, network),
            (
                &LhsValue::IpAddr(IpAddr::V6(ref addr)),
                &RhsValue::IpCidr(IpCidr::V6(ref network)),
            ) => ip_order(addr, network),
            (&LhsValue::Unsigned(lhs), &RhsValue::Unsigned(ref rhs)) => lhs.cmp(rhs),
            (&LhsValue::Bytes(ref lhs), &RhsValue::Bytes(ref rhs)) => lhs.cmp(rhs),
            _ => return None,
        })
    }
}

fn exec_op(lhs: &LhsValue, op: ::op::ComparisonOp, rhs: RhsValue) -> Option<bool> {
    use op::ComparisonOp::*;
    use op::MatchingOp;

    match op {
        Ordering(op) => lhs.partial_cmp(&rhs)
            .map(|ordering| op.contains(ordering.into())),

        Matching(op) => Some(match (lhs, op, rhs) {
            (&LhsValue::Bytes(ref lhs), MatchingOp::Matches, RhsValue::Bytes(ref rhs)) => {
                match (lhs.as_str(), rhs.as_str()) {
                    (Some(lhs), Some(rhs)) => Regex::new(rhs).unwrap().is_match(lhs),
                    _ => return None,
                }
            }
            (&LhsValue::Unsigned(lhs), MatchingOp::BitwiseAnd, RhsValue::Unsigned(rhs)) => {
                (lhs & rhs) != 0
            }
            (&LhsValue::Bytes(ref lhs), MatchingOp::Contains, RhsValue::Bytes(ref rhs)) => {
                lhs.contains(rhs)
            }
            _ => return None,
        }),
    }
}

impl<'i> Context<'i> for &'i ExecutionContext {
    type LhsValue = &'i LhsValue;
    type Filter = bool;

    fn get_field(self, path: &str) -> Option<&'i LhsValue> {
        self.0.get(path)
    }

    fn compare(self, lhs: &LhsValue, op: ::op::ComparisonOp, rhs: RhsValue) -> Result<bool, Type> {
        exec_op(lhs, op, rhs).ok_or_else(|| match *lhs {
            LhsValue::IpAddr(IpAddr::V4(_)) => Type::IpAddrV4,
            LhsValue::IpAddr(IpAddr::V6(_)) => Type::IpAddrV6,
            LhsValue::Bytes(ref b) => if b.is_str() {
                Type::String
            } else {
                Type::Bytes
            },
            LhsValue::Unsigned(_) => Type::Unsigned,
        })
    }

    fn one_of<I: Iterator<Item = RhsValue>>(self, lhs: &LhsValue, rhs: I) -> Result<bool, Type> {
        let mut acc = true;
        for rhs in rhs {
            acc |= self.compare(
                lhs,
                ::op::ComparisonOp::Ordering(::op::OrderingMask::EQUAL),
                rhs,
            )?;
        }
        Ok(acc)
    }
}

impl Filter for bool {
    fn combine(self, op: ::op::CombiningOp, other: bool) -> bool {
        use op::CombiningOp::*;

        match op {
            And => self && other,
            Or => self || other,
            Xor => self != other,
        }
    }

    fn unary(self, op: ::op::UnaryOp) -> bool {
        use op::UnaryOp::*;

        match op {
            Not => !self,
        }
    }
}
