use std::sync::atomic::{AtomicU8, Ordering};

// Access
const RELEASED: u8 = 0b0000_0000;
const ACCESSED: u8 = 0b0000_0001;
const TAKEN: u8 = 0b0000_0010;
const DECLARE_TAKE: u8 = 0b0000_0011;

// Identity
const NODE: u8 = 0b0000_0000;
const FRONT: u8 = 0b0001_0000;
const BACK: u8 = 0b0010_0000;

const ACCESS_MASK: u8 = 0b0000_1111;
const IDENT_MASK: u8 = 0b1111_0000;

#[derive(Debug, PartialEq, Eq)]
pub enum Identity {
    Front,
    Back,
    Node,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Status {
    Released,
    Accessed,
    Taken,
    DeclareTake,
}

// INVARIANT: ident bits never change
#[derive(Debug)]
pub struct AccessFlag {
    flag: AtomicU8,
}

impl AccessFlag {
    pub const fn new(identity: &Identity) -> Self {
        let ident = match identity {
            Identity::Front => FRONT,
            Identity::Back => BACK,
            Identity::Node => NODE,
        };
        Self {
            flag: AtomicU8::new(ident | RELEASED),
        }
    }

    pub fn try_access<'a>(&'a self) -> Result<ReleaseGuard<'a>, Status> {
        let ident_bits = self.ident_bits();

        match self.flag.compare_exchange(
            RELEASED | ident_bits,
            ACCESSED | ident_bits,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => Ok(ReleaseGuard { flag: &self }),
            Err(f) => {
                let access_status = f & ACCESS_MASK;
                if access_status == ACCESSED {
                    Err(Status::Accessed)
                } else if access_status == DECLARE_TAKE {
                    Err(Status::DeclareTake)
                } else if access_status == TAKEN {
                    // taken values should be guarded
                    // in this case, they represent a node which is about to be dropped
                    // which includes this access flag
                    unreachable!("reading an AccessFlag after it was taken");
                } else {
                    unreachable!("impossible or unhandled flag {:08b}", f);
                }
            }
        }
    }

    pub fn try_declare_take(&self) -> Result<(), Status> {
        let ident_bits = self.ident_bits();

        match self.flag.compare_exchange(
            RELEASED | ident_bits,
            DECLARE_TAKE | ident_bits,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => Ok(()),
            Err(f) => {
                let access_status = f & ACCESS_MASK;
                if access_status == ACCESSED {
                    Err(Status::Accessed)
                } else if access_status == DECLARE_TAKE {
                    unreachable!("a flag is being set to DECLARE_TAKE more than once");
                } else if access_status == TAKEN {
                    // taken values should be guarded
                    // in this case, they represent a node which is about to be dropped
                    // which includes this access flag
                    unreachable!("reading an AccessFlag after it was taken");
                } else {
                    unreachable!("impossible or unhandled flag {:08b}", f);
                }
            }
        }
    }

    // TODO: Make it mandatory to hand in a "DeclaredTakeGuard"
    pub fn try_take(&self) -> Result<(), Status> {
        let identity = self.identity();
        if identity == Identity::Front || identity == Identity::Back {
            unreachable!(
                "attempted to take {:08b} with identity {:?}",
                self.flag.load(Ordering::SeqCst),
                identity
            );
        }
        match self.flag.compare_exchange(
            DECLARE_TAKE | NODE,
            TAKEN | NODE,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => Ok(()),
            Err(f) => {
                let access_status = f & ACCESS_MASK;
                if access_status == ACCESSED {
                    Err(Status::Accessed)
                } else if access_status == TAKEN {
                    // taken values should be guarded
                    // in this case, they represent a node which is about to be dropped
                    // which includes this access flag
                    unreachable!("reading an AccessFlag after it was taken");
                } else if access_status == RELEASED {
                    unreachable!(
                        "attempted to take before declaring intent (use try_declare_take)"
                    );
                } else {
                    unreachable!("impossible or unhandled flag {:08b}", f);
                }
            }
        }
    }

    fn ident_bits(&self) -> u8 {
        self.flag.load(Ordering::SeqCst) & IDENT_MASK
    }

    const fn ident_from_bits(bits: u8) -> Identity {
        match bits {
            NODE => Identity::Node,
            FRONT => Identity::Front,
            BACK => Identity::Back,
            _ => unreachable!(),
        }
    }

    pub fn identity(&self) -> Identity {
        Self::ident_from_bits(self.ident_bits())
    }

    fn status_bits(&self) -> u8 {
        self.flag.load(Ordering::SeqCst) & ACCESS_MASK
    }

    const fn status_from_bits(bits: u8) -> Status {
        match bits {
            RELEASED => Status::Released,
            TAKEN => Status::Taken,
            ACCESSED => Status::Accessed,
            DECLARE_TAKE => Status::DeclareTake,
            _ => unreachable!(),
        }
    }

    pub fn status(&self) -> Status {
        Self::status_from_bits(self.status_bits())
    }
}

pub struct ReleaseGuard<'a> {
    flag: &'a AccessFlag,
}

impl<'a> ReleaseGuard<'a> {
    pub fn release(&self) {
        let ident = self.flag.ident_bits();
        match self.flag.flag.compare_exchange(
            ACCESSED | ident,
            RELEASED | ident,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => {}
            Err(f) => {
                let access_status = f & ACCESS_MASK;
                if access_status == RELEASED {
                    return;
                }
                unreachable!("could not release an accessed resource: flag was {:08b}", f);
            }
        }
    }
}

impl<'a> Drop for ReleaseGuard<'a> {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_identity() {
        let init_ident = Identity::Node;
        let node = AccessFlag::new(&init_ident);
        let guard = loop {
            match node.try_access() {
                Ok(g) => break g,
                Err(_) => {}
            }
        };
        assert_eq!(init_ident, node.identity());
        drop(guard);
        assert_eq!(init_ident, node.identity());
        while let Err(_) = node.try_take() {}
        assert_eq!(init_ident, node.identity());
    }

    #[test]
    fn valid_status_lifecycle() {
        let init_ident = Identity::Node;
        let node = AccessFlag::new(&init_ident);
        assert_eq!(Status::Released, node.status());

        let guard = node
            .try_access()
            .expect("could not access flag while released");
        assert_eq!(Status::Accessed, node.status());

        drop(guard);
        assert_eq!(Status::Released, node.status());

        node.try_take().expect("could not take flag while released");
        assert_eq!(Status::Taken, node.status());
    }

    #[test]
    fn blocks_access() {
        let init_ident = Identity::Node;
        let node = AccessFlag::new(&init_ident);

        let guard = node
            .try_access()
            .expect("could not access flag while released");

        assert!(node.try_access().is_err());

        drop(guard);

        assert!(node.try_access().is_ok());
    }

    #[test]
    #[should_panic]
    fn take_front_panics() {
        let init_ident = Identity::Front;
        let node = AccessFlag::new(&init_ident);
        let _ = node.try_take();
    }

    #[test]
    #[should_panic]
    fn take_back_panics() {
        let init_ident = Identity::Back;
        let node = AccessFlag::new(&init_ident);
        let _ = node.try_take();
    }
}
