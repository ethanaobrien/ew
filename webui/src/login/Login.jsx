import { useState } from 'react'
import './Login.css'
import Request from '../Request.jsx'

function Login() {
    const error = useState(new URL(window.location).searchParams.get("message") || "");
    const uid = useState((window.localStorage && window.localStorage.getItem("ew_uid")) || "");
    let password;

    const handleSubmit = async (event) => {
        event.preventDefault();
        if (!uid[0] || !password || isNaN(uid[0])) return;
        if (window.localStorage) window.localStorage.setItem("ew_uid", uid[0]);
        let resp = await Request(
            "/api/webui/login",
            {
                uid: parseInt(uid[0]),
                password: password
            }
        );
        if (resp.result == "OK") {
            window.location.href = "/home/";
        } else {
            error[1](resp.message);
        }
    };
  
    return (
        <div id="login-form">
            <h1>Login</h1>
            <form>
                <label htmlFor="id">SIF2 ID:</label>
                <input type="text" id="id" name="id" onChange={(event) => {uid[1](event.target.value)}} value={uid[0]} />
                <label htmlFor="password">Transfer passcode:</label>
                <input type="password" id="password" name="password" onChange={(event) => {password = event.target.value}} />
                <input type="submit" value="Submit" onClick={handleSubmit}/>
                { error[0] && <p id="error">Error: { error[0] } </p> }
            </form>
        </div>
    );
}

export default Login;
