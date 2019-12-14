use std::pin::Pin;
use std::task::{Context, Poll};
use futures::stream::Stream;

use tokio::sync::mpsc;

use std::hash::Hash;

use std::collections::HashMap;
use std::collections::HashSet;

use crate::sse::Event;

pub struct Rooms<R: 'static + Eq + Hash + Clone + Send + Sync, U: 'static + Eq + Hash + Clone + Send + Sync> {
    tx: mpsc::Sender<Command<R, U>>
}

enum Command<R: 'static + Eq + Hash + Clone + Send + Sync, U: 'static + Eq + Hash + Clone + Send + Sync> {
    Subscribe { user: U, tx: mpsc::Sender<Event> },
    Join { user: U, room: R },
    Leave { user: U, room: R },
    SendMessage { room: R, message: Event }
}

pub struct Subscription(mpsc::Receiver<Event>);

impl<R: 'static + Eq + Hash + Clone + Send + Sync, U: 'static + Eq + Hash + Clone + Send + Sync> Rooms<R, U> {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(1024); // what number?
        tokio::spawn(Self::background_task(rx));
        Rooms { tx }
    }

    pub async fn subscribe(&self, user: U) -> Subscription {
        let (tx, rx) = mpsc::channel(10);

        self.tx.clone().send(Command::Subscribe { user, tx }).await;

        Subscription(rx)
    }

    pub async fn join(&self, room: R, user: U) {
        self.tx.clone().send(Command::Join { room, user }).await;
    }

    pub async fn leave(&self, room: R, user: U) {
        self.tx.clone().send(Command::Leave { room, user }).await;
    }

    pub async fn send(&self, room: R, message: Event) {
        self.tx.clone().send(Command::SendMessage { room, message }).await;
    }
    
    fn helper_subscribe(users_to_subscriptions: &mut HashMap<U, mpsc::Sender<Event>>, user: &U, tx: mpsc::Sender<Event>) {
        users_to_subscriptions.insert(user.clone(), tx);
    }

    fn helper_join(rooms_to_users: &mut HashMap<R, HashSet<U>>, users_to_rooms: &mut HashMap<U, HashSet<R>>, room: &R, user: &U) {
        let mut users_set = rooms_to_users.entry(room.clone()).or_insert(HashSet::new());
        users_set.insert(user.clone());

        let mut rooms_set = users_to_rooms.entry(user.clone()).or_insert(HashSet::new());
        rooms_set.insert(room.clone());
    }

    fn helper_leave(rooms_to_users: &mut HashMap<R, HashSet<U>>, users_to_rooms: &mut HashMap<U, HashSet<R>>, room: &R, user: &U) {
        if let Some(mut users_set) = rooms_to_users.get_mut(room) {
            users_set.remove(user);
        }

        if let Some(mut rooms_set) = users_to_rooms.get_mut(user) {
            rooms_set.remove(room);
        }
    }

    async fn helper_send(users_to_subscriptions: &mut HashMap<U, mpsc::Sender<Event>>, rooms_to_users: &mut HashMap<R, HashSet<U>>, users_to_rooms: &mut HashMap<U, HashSet<R>>, room: &R, message: Event) { 
        if let Some(room) = rooms_to_users.get(room) {
            let mut disconnects = vec![];

            for user in room.iter() {
                if let Some(mut sender) = users_to_subscriptions.get_mut(user) {
                    if sender.send(message.clone()).await.is_err() {
                        disconnects.push(user);
                    }
                }
            }

            /* got some borrow errors so commenting out in the meantime
            // Remove any disconnects from hashmaps
            for user in disconnects {
                users_to_subscriptions.remove(user);

                if let Some(member_rooms) = users_to_rooms.remove(user) {
                    for r in member_rooms.iter() {
                        if let Some(mut the_room) = rooms_to_users.get_mut(r) {
                            the_room.remove(user);
                        }
                    }
                }
            }
            */
        }
    }

    async fn background_task(mut rx: mpsc::Receiver<Command<R, U>>) {
        let mut users_to_rooms: HashMap<U, HashSet<R>> = HashMap::new();
        let mut rooms_to_users: HashMap<R, HashSet<U>> = HashMap::new();
        let mut users_to_subscriptions: HashMap<U, mpsc::Sender<Event>> = HashMap::new();

        while let Some(command) = rx.recv().await {
            match command {
                Command::Subscribe { user, tx } => {
                    Self::helper_subscribe(&mut users_to_subscriptions, &user, tx);
                }
                Command::Join { room, user } => {
                    Self::helper_join(&mut rooms_to_users, &mut users_to_rooms, &room, &user);
                }
                Command::Leave { room, user } => {
                    Self::helper_leave(&mut rooms_to_users, &mut users_to_rooms, &room, &user);
                }
                Command::SendMessage { room, message } => {
                    Self::helper_send(&mut users_to_subscriptions, &mut rooms_to_users, &mut users_to_rooms, &room, message).await;
                }
                _ => {
                    todo!()
                }
            }
        }
    }
}

impl Stream for Subscription {
    type Item = Event;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0).poll_recv(cx)
    }
}