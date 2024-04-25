import { useState, useParams, useEffect } from 'react'
import './Home.css'
import Request from '../Request.jsx'

function Home() {
    const [user, userdata] = useState();
    
    const logout = () => {
        window.location.href = "/webui/logout";
    }
    
    useEffect(() => {
        if (user) return;
        (async () => {
            let resp = await Request("/api/webui/userInfo");
            if (resp.result !== "OK") {
                window.location.href = "/?message=" + encodeURIComponent(resp.message);
                return;
            }
            let user = resp.data.userdata;
            //let login = resp.data.loginbonus;
            userdata(
                <div>
                    <p>User id: { user.user.id } </p>
                    <p>Rank: { user.user.rank } ({ user.user.exp } exp)</p>
                    <p>Last Login: { (new Date(user.user.last_login_time * 1000)).toString() } </p>
                </div>
            );
        })();
    });
  
    return (
        <div id="home">
            <button id="logout" onClick={logout}>Logout</button>
            <h1>Home</h1>
            
            { user ? <div> { user } </div> : <p>Loading...</p> }
        </div>
    );
}

export default Home;
