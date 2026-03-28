use axum::response::Html;

pub fn login_page() -> Html<String> {
    Html(r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>Вход</title>
</head>
<body>
    <h1>Вход</h1>
    <form id="loginForm">
        <input type="text" id="username" placeholder="Username" required><br>
        <input type="password" id="password" placeholder="Password" required><br>
        <button type="submit">Войти</button>
    </form>
    <p><a href="/register-page">Регистрация</a></p>
    <script>
        document.getElementById('loginForm').onsubmit = async (e) => {
            e.preventDefault();
            const username = document.getElementById('username').value;
            const password = document.getElementById('password').value;
            
            const res = await fetch('/login', {
                method: 'POST',
                headers: {'Content-Type': 'application/json'},
                body: JSON.stringify({username, password})
            });
            
            const data = await res.json();
            if (data.success) {
                localStorage.setItem('currentUser', username);
                window.location.href = '/';
            } else {
                alert(data.message);
            }
        };
    </script>
</body>
</html>
"#.to_string())
}

pub fn register_page() -> Html<String> {
    Html(r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>Регистрация</title>
</head>
<body>
    <h1>Регистрация</h1>
    <form id="registerForm">
        <input type="text" id="username" placeholder="Username" required><br>
        <input type="password" id="password" placeholder="Password" required><br>
        <input type="date" id="birth_date" required><br>
        <input type="email" id="email" placeholder="Email" required><br>
        <textarea id="description" placeholder="Описание"></textarea><br>
        <button type="submit">Зарегистрироваться</button>
    </form>
    <p><a href="/login-page">Войти</a></p>
    <script>
        document.getElementById('registerForm').onsubmit = async (e) => {
            e.preventDefault();
            const username = document.getElementById('username').value;
            const password = document.getElementById('password').value;
            const birth_date = document.getElementById('birth_date').value;
            const email = document.getElementById('email').value;
            const description = document.getElementById('description').value;
            
            const res = await fetch('/register', {
                method: 'POST',
                headers: {'Content-Type': 'application/json'},
                body: JSON.stringify({username, password, birth_date, email, description})
            });
            
            const data = await res.json();
            alert(data.message);
            if (data.success) {
                window.location.href = '/login-page';
            }
        };
    </script>
</body>
</html>
"#.to_string())
}

pub fn home_page() -> Html<String> {
    Html(r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>Главная</title>
</head>
<body>
    <div>
        <h1>Messenger</h1>
        <div>
            <span id="userName"></span>
            <button onclick="goToProfile()">Мой профиль</button>
            <button onclick="logout()">Выйти</button>
        </div>
    </div>
    
    <div>
        <h2>Поиск</h2>
        <input type="text" id="searchInput" placeholder="Имя пользователя...">
        <button onclick="searchUsers()">Найти</button>
        <div id="searchResults"></div>
    </div>
    
    <div>
        <h2>Мои чаты</h2>
        <div id="chatsList"></div>
    </div>
    
    <div id="chatWindow" style="display:none; border:1px solid #ccc; padding:10px; margin-top:20px;">
        <h3>Чат с <span id="chatWith"></span></h3>
        <div id="messages" style="height:300px; overflow-y:scroll; border:1px solid #eee; padding:5px;"></div>
        <input type="text" id="messageInput" placeholder="Сообщение...">
        <button onclick="sendMessage()">Отправить</button>
        <button onclick="closeChat()">Закрыть</button>
    </div>

    <script>
        let currentUser = localStorage.getItem('currentUser');
        let currentChat = null;
        
        if (!currentUser) {
            window.location.href = '/login-page';
        }
        
        document.getElementById('userName').innerText = currentUser;
        
        async function loadChats() {
            const res = await fetch(`/api/chats?username=${currentUser}`);
            const data = await res.json();
            
            if (data.success && data.chats.length > 0) {
                document.getElementById('chatsList').innerHTML = data.chats.map(chat => `
                    <div style="border:1px solid #ccc; padding:5px; margin:5px; cursor:pointer;" onclick="openChat('${chat.user}')">
                        <strong>${chat.user}</strong><br>
                        <small>${chat.last_message}</small>
                    </div>
                `).join('');
            } else {
                document.getElementById('chatsList').innerHTML = '<p>Нет чатов</p>';
            }
        }
        
        async function searchUsers() {
            const query = document.getElementById('searchInput').value;
            if (!query.trim()) return;
            
            const res = await fetch(`/api/search?q=${encodeURIComponent(query)}&current=${currentUser}`);
            const data = await res.json();
            
            if (data.success && data.users.length > 0) {
                document.getElementById('searchResults').innerHTML = data.users.map(user => `
                    <div style="border:1px solid #ccc; padding:5px; margin:5px; cursor:pointer;" onclick="openChat('${user.username}')">
                        <strong>${user.username}</strong>
                        <p>${user.description || ''}</p>
                    </div>
                `).join('');
            } else {
                document.getElementById('searchResults').innerHTML = '<p>Не найдено</p>';
            }
        }
        
        async function openChat(username) {
            currentChat = username;
            document.getElementById('chatWith').innerText = username;
            document.getElementById('chatWindow').style.display = 'block';
            await loadMessages();
        }
        
        async function loadMessages() {
            const res = await fetch(`/messages/${currentUser}/${currentChat}`);
            const data = await res.json();
            
            if (data.success) {
                document.getElementById('messages').innerHTML = data.messages.map(msg => `
                    <div style="text-align: ${msg.from === currentUser ? 'right' : 'left'}; margin:5px;">
                        <strong>${msg.from}:</strong> ${msg.content}<br>
                        <small>${msg.time}</small>
                    </div>
                `).join('');
                document.getElementById('messages').scrollTop = document.getElementById('messages').scrollHeight;
            }
        }
        
        async function sendMessage() {
            const content = document.getElementById('messageInput').value;
            if (!content.trim()) return;
            
            const res = await fetch('/message', {
                method: 'POST',
                headers: {'Content-Type': 'application/json'},
                body: JSON.stringify({
                    from: currentUser,
                    to: currentChat,
                    content: content
                })
            });
            
            const data = await res.json();
            if (data.success) {
                document.getElementById('messageInput').value = '';
                await loadMessages();
                await loadChats();
            } else {
                alert(data.message);
            }
        }
        
        function closeChat() {
            document.getElementById('chatWindow').style.display = 'none';
            currentChat = null;
        }
        
        function goToProfile() {
            window.location.href = `/profile/${currentUser}`;
        }
        
        async function logout() {
            await fetch('/logout', {method: 'POST'});
            localStorage.removeItem('currentUser');
            window.location.href = '/login-page';
        }
        
        loadChats();
        
        setInterval(() => {
            if (currentChat) {
                loadMessages();
            }
            loadChats();
        }, 3000);
    </script>
</body>
</html>
"#.to_string())
}

pub fn profile_page(username: &str) -> Html<String> {
    Html(format!(r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>Профиль</title>
</head>
<body>
    <button onclick="location.href='/'">На главную</button>
    <div id="profile"></div>
    
    <div id="editForm" style="display:none; border:1px solid #ccc; padding:10px; margin-top:20px;">
        <h3>Редактировать</h3>
        <form id="updateForm">
            <input type="date" id="edit_birth_date"><br>
            <input type="email" id="edit_email"><br>
            <textarea id="edit_description" rows="3"></textarea><br>
            <button type="submit">Сохранить</button>
            <button type="button" onclick="hideEdit()">Отмена</button>
        </form>
        
        <h4>Сменить аватарку</h4>
        <form id="avatarForm" enctype="multipart/form-data">
            <input type="file" id="avatar" name="avatar" accept="image/*"><br>
            <button type="submit">Загрузить</button>
        </form>
    </div>
    
    <button id="editBtn" onclick="showEdit()">Редактировать</button>

    <script>
        const username = '{}';
        const currentUser = localStorage.getItem('currentUser');
        let canEdit = false;
        
        async function loadProfile() {{
            const res = await fetch(`/api/profile/${{username}}?current=${{currentUser}}`);
            const data = await res.json();
            
            if (data.success) {{
                const p = data.profile;
                canEdit = data.can_edit;
                
                document.getElementById('profile').innerHTML = `
                    <h1>${{p.username}}</h1>
                    <img src="${{p.avatar_path || ''}}" width="150" height="150" style="border-radius:50%; object-fit:cover"><br>
                    <p><strong>Дата рождения:</strong> ${{p.birth_date}}</p>
                    <p><strong>Email:</strong> ${{p.email}}</p>
                    <p><strong>Описание:</strong> ${{p.description || 'Нет описания'}}</p>
                    <p><strong>Зарегистрирован:</strong> ${{p.created_at}}</p>
                `;
                
                if (canEdit) {{
                    document.getElementById('editBtn').style.display = 'inline-block';
                    document.getElementById('edit_birth_date').value = p.birth_date;
                    document.getElementById('edit_email').value = p.email;
                    document.getElementById('edit_description').value = p.description || '';
                }} else {{
                    document.getElementById('editBtn').style.display = 'none';
                }}
            }}
        }}
        
        function showEdit() {{
            if (canEdit) {{
                document.getElementById('editForm').style.display = 'block';
            }}
        }}
        
        function hideEdit() {{
            document.getElementById('editForm').style.display = 'none';
        }}
        
        document.getElementById('updateForm').onsubmit = async (e) => {{
            e.preventDefault();
            const birth_date = document.getElementById('edit_birth_date').value;
            const email = document.getElementById('edit_email').value;
            const description = document.getElementById('edit_description').value;
            
            const res = await fetch('/api/profile/update', {{
                method: 'POST',
                headers: {{'Content-Type': 'application/json'}},
                body: JSON.stringify({{username, birth_date, email, description, current_user: currentUser}})
            }});
            
            const data = await res.json();
            alert(data.message);
            if (data.success) {{
                loadProfile();
                hideEdit();
            }}
        }};
        
        document.getElementById('avatarForm').onsubmit = async (e) => {{
            e.preventDefault();
            const formData = new FormData();
            formData.append('username', username);
            formData.append('current_user', currentUser);
            formData.append('avatar', document.getElementById('avatar').files[0]);
            
            const res = await fetch('/api/profile/avatar', {{
                method: 'POST',
                body: formData
            }});
            
            const data = await res.json();
            alert(data.message);
            if (data.success) {{
                loadProfile();
            }}
        }};
        
        loadProfile();
    </script>
</body>
</html>
"#, username))
}