import { useState } from 'react'
import './Login.css'
import Request from '../Request.jsx'

function Login() {
    const error = useState(new URL(window.location).searchParams.get("message") || "");
    const uid = useState((window.localStorage && window.localStorage.getItem("ew_uid")) || "");
    const password = useState("");

    function showError(message) {
        error[1](message);
    }

    const handleSubmit = async (event) => {
        event.preventDefault();
        if (!uid[0] || !password[0]) {
            showError("Missing userid/password");
            return;
        }
        if (isNaN(uid[0])) {
            showError("UserID should be a number. (The \"Friend ID\" in your profile)");
            return;
        }
        if (window.localStorage) window.localStorage.setItem("ew_uid", uid[0]);
        let resp = await Request(
            "/api/webui/login",
            {
                uid: parseInt(uid[0]),
                password: password[0]
            }
        );
        if (resp.result == "OK") {
            window.location.href = "/home/";
        } else {
            showError(resp.message);
        }
    };
    
    const import_user = (e) => {
        e.preventDefault();
        window.location.href = "/import/";
    }

    const help = (e) => {
        e.preventDefault();
        window.location.href = "/help/";
    }
  
    return (
        <div id="login-form">
            <h1>Login</h1>
            <form>
                <label htmlFor="id">SIF2 ID:</label>
                <input type="text" id="id" name="id" onChange={(event) => {uid[1](event.target.value)}} value={uid[0]} />
                <label htmlFor="password">Transfer passcode:</label>
                <input type="password" id="password" name="password" onChange={(event) => {password[1](event.target.value)}} />
                <input type="submit" value="Submit" onClick={handleSubmit}/>
                <div id="sub_div">
                    <button onClick={import_user}>Import User</button><br/><br/>
                    <button onClick={help}>Need help?</button><br/><br/>
                    { error[0] ? <p>Error: { error[0] } </p> : <p></p> }
                </div>
                <p>EW Version 1.0.0 - <a href="https://git.ethanthesleepy.one/ethanaobrien/ew">View source</a> - <a href="https://git.ethanthesleepy.one/ethanaobrien/ew/src/branch/main/LICENSE">View license</a></p>
            </form>
        </div>
    );
}

export default Login;
