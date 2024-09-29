#[macro_use]
extern crate serde;
use candid::{Decode, Encode};
use ic_cdk::api::time;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{BoundedStorable, Cell, DefaultMemoryImpl, StableBTreeMap, Storable};
use std::{borrow::Cow, cell::RefCell};

type Memory = VirtualMemory<DefaultMemoryImpl>;
type IdCell = Cell<u64, Memory>;

#[derive(candid::CandidType, Clone, Serialize, Deserialize, Default)]
struct User {
    username: String,
    id: u64,
    created_at: u64,
}

#[derive(candid::CandidType, Clone, Serialize, Deserialize, Default)]
struct Discussion {
    id: u64,
    topic: String,
    created_by: String,
    created_at: u64,
    upvotes: u64,
    downvotes: u64,
}

#[derive(candid::CandidType, Clone, Serialize, Deserialize, Default)]
struct Vote {
    id: u64,
    by: String,
    discussion_id: u64,
    vote_type: VoteType,
    created_at: u64,
}

#[derive(candid::CandidType, Clone, Serialize, Deserialize)]
enum VoteType {
    Upvote,
    Downvote,
}

impl Default for VoteType {
    fn default() -> Self {
        VoteType::Upvote
    }
}

impl Storable for User {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }
}

impl BoundedStorable for User {
    const MAX_SIZE: u32 = 512;
    const IS_FIXED_SIZE: bool = false;
}

impl Storable for Discussion {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }
}

impl BoundedStorable for Discussion {
    const MAX_SIZE: u32 = 512;
    const IS_FIXED_SIZE: bool = false;
}

impl Storable for Vote {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }
}

impl BoundedStorable for Vote {
    const MAX_SIZE: u32 = 512;
    const IS_FIXED_SIZE: bool = false;
}

// Thread-local storage for the memory manager and data storage
thread_local! {
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> = RefCell::new(
        MemoryManager::init(DefaultMemoryImpl::default())
    );
    static ID_COUNTER: RefCell<IdCell> = RefCell::new(
        IdCell::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0))), 0).expect("Cannot create a counter")
    );
    static USERS_STORAGE: RefCell<StableBTreeMap<u64, User, Memory>> = RefCell::new(
        StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1))))
    );
    static DISCUSSIONS_STORAGE: RefCell<StableBTreeMap<u64, Discussion, Memory>> = RefCell::new(
        StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(2))))
    );
    static VOTES_STORAGE: RefCell<StableBTreeMap<u64, Vote, Memory>> = RefCell::new(
        StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(4))))
    );
}

// Function to register a user
#[ic_cdk::update]
fn register_user(username: String) -> Result<User, String> {
    if username.is_empty() {
        return Err("Username is required".to_string());
    }

    if is_user_registered(&username) {
        return Err("Username already exists".to_string());
    }

    let id = ID_COUNTER.with(|counter| {
        let current_value = *counter.borrow().get();
        counter.borrow_mut().set(current_value + 1)
    }).expect("Cannot increment ID counter");

    let new_user = User {
        username: username.clone(),
        id,
        created_at: time(),
    };

    USERS_STORAGE.with(|storage| storage.borrow_mut().insert(id, new_user.clone()));

    Ok(new_user)
}

// Helper function to check if a user is registered
fn is_user_registered(username: &String) -> bool {
    USERS_STORAGE.with(|storage| {
        storage.borrow().iter().any(|(_, user)| user.username == *username)
    })
}

// Function to create a new discussion with user validation
#[ic_cdk::update]
fn create_discussion(topic: String, username: String) -> Result<Discussion, String> {
    if topic.is_empty() {
        return Err("Topic is required".to_string());
    }

    // Validate if user is registered
    if !is_user_registered(&username) {
        return Err("User is not registered".to_string());
    }

    let id = ID_COUNTER.with(|counter| {
        let current_value = *counter.borrow().get();
        counter.borrow_mut().set(current_value + 1)
    }).expect("Cannot increment ID counter");

    let discussion = Discussion {
        id,
        topic,
        created_by: username,
        created_at: time(),
        upvotes: 0,
        downvotes: 0,
    };

    DISCUSSIONS_STORAGE.with(|storage| storage.borrow_mut().insert(id, discussion.clone()));

    Ok(discussion)
}

// New function to allow discussion topic edit (only by creator)
#[ic_cdk::update]
fn edit_discussion(discussion_id: u64, new_topic: String, username: String) -> Result<String, String> {
    if new_topic.is_empty() {
        return Err("New topic cannot be empty".to_string());
    }

    let mut discussion = DISCUSSIONS_STORAGE.with(|storage| {
        storage.borrow().get(&discussion_id).map(|d| d.clone())
    }).ok_or("Discussion not found")?;

    if discussion.created_by != username {
        return Err("Only the creator can edit the discussion".to_string());
    }

    discussion.topic = new_topic;

    DISCUSSIONS_STORAGE.with(|storage| storage.borrow_mut().insert(discussion_id, discussion));

    Ok("Discussion topic updated".to_string())
}

// Function to vote on a discussion
#[ic_cdk::update]
fn vote_discussion(vote_type: VoteType, discussion_id: u64, username: String) -> Result<String, String> {
    if !is_user_registered(&username) {
        return Err("User is not registered".to_string());
    }

    let user_has_voted = VOTES_STORAGE.with(|storage| {
        storage.borrow().iter().any(|(_, vote)| vote.by == username && vote.discussion_id == discussion_id)
    });

    if user_has_voted {
        return Err("User has already voted on this discussion".to_string());
    }

    let id = ID_COUNTER.with(|counter| {
        let current_value = *counter.borrow().get();
        counter.borrow_mut().set(current_value + 1)
    }).expect("Cannot increment ID counter");

    let vote = Vote {
        id,
        by: username.clone(),
        discussion_id,
        vote_type: vote_type.clone(),
        created_at: time(),
    };

    VOTES_STORAGE.with(|storage| storage.borrow_mut().insert(id, vote));

    let updated_discussion = DISCUSSIONS_STORAGE.with(|storage| {
        storage.borrow().get(&discussion_id).map(|d| d.clone())
    });

    if let Some(mut discussion) = updated_discussion {
        match vote_type {
            VoteType::Upvote => discussion.upvotes += 1,
            VoteType::Downvote => discussion.downvotes += 1,
        }

        DISCUSSIONS_STORAGE.with(|storage| {
            storage.borrow_mut().insert(discussion_id, discussion);
        });

        Ok("Vote recorded for discussion".to_string())
    } else {
        Err("Discussion not found".to_string())
    }
}

// New function to remove a vote from a discussion
#[ic_cdk::update]
fn remove_vote(discussion_id: u64, username: String) -> Result<String, String> {
    if !is_user_registered(&username) {
        return Err("User is not registered".to_string());
    }

    let vote = VOTES_STORAGE.with(|storage| {
        storage.borrow().iter().find(|(_, vote)| vote.by == username && vote.discussion_id == discussion_id).map(|(_, v)| v.clone())
    }).ok_or("Vote not found")?;

    VOTES_STORAGE.with(|storage| storage.borrow_mut().remove(&vote.id));

    let mut discussion = DISCUSSIONS_STORAGE.with(|storage| {
        storage.borrow().get(&discussion_id).map(|d| d.clone())
    }).ok_or("Discussion not found")?;

    match vote.vote_type {
        VoteType::Upvote => discussion.upvotes -= 1,
        VoteType::Downvote => discussion.downvotes -= 1,
    }

    DISCUSSIONS_STORAGE.with(|storage| storage.borrow_mut().insert(discussion_id, discussion));

    Ok("Vote removed".to_string())
}

// Function to delete a user and associated data
#[ic_cdk::update]
fn delete_user(username: String) -> Result<String, String> {
    if !is_user_registered(&username) {
        return Err("User not found".to_string());
    }

    let user_id = USERS_STORAGE.with(|storage| {
        storage.borrow().iter().find(|(_, user)| user.username == username).map(|(id, _)| id)
    }).ok_or("User not found")?;

    // Remove the user
    USERS_STORAGE.with(|storage| storage.borrow_mut().remove(&user_id));

    // Remove all votes and update discussions
    VOTES_STORAGE.with(|storage| {
        let votes: Vec<u64> = storage.borrow().iter()
            .filter(|(_, vote)| vote.by == username)
            .map(|(id, _)| id)
            .collect();

        let mut storage_mut = storage.borrow_mut();  // Mutable borrow happens here once, outside the loop
        for vote_id in votes {
            storage_mut.remove(&vote_id);
        }
    });

    // Remove discussions created by the user (or mark them as anonymous)
    DISCUSSIONS_STORAGE.with(|storage| {
        let keys_to_update: Vec<u64> = storage.borrow().iter()
        .filter(|(_, discussion)| discussion.created_by == username)
        .map(|(id, _)| id)
        .collect();

        let mut storage_mut = storage.borrow_mut();
        for id in keys_to_update {
            if let Some(mut discussion) = storage_mut.remove(&id) {
                discussion.created_by = "Anonymous".to_string();
                storage_mut.insert(id, discussion); // Reinsert the modified discussion
            }
        }
    });
    
    Ok("User and associated data deleted".to_string())
}

// Function to get all discussions
#[ic_cdk::query]
fn get_discussions() -> Vec<Discussion> {
    DISCUSSIONS_STORAGE.with(|storage| {
        storage.borrow().iter().map(|(_, discussion)| discussion).collect()
    })
}

// Function to get all users
#[ic_cdk::query]
fn get_users() -> Vec<User> {
    USERS_STORAGE.with(|storage| {
        storage.borrow().iter().map(|(_, user)| user).collect()
    })
}

// Function to get total vote count for a discussion
#[ic_cdk::query]
fn get_vote_count(discussion_id: u64) -> Result<(u64, u64), String> {
    let discussion = DISCUSSIONS_STORAGE.with(|storage| {
        storage.borrow().get(&discussion_id).map(|d| d.clone())
    }).ok_or("Discussion not found")?;

    Ok((discussion.upvotes, discussion.downvotes))
}

ic_cdk::export_candid!();
