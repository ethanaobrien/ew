import { useState, useParams, useEffect } from 'react'
import './Home.css'
import Request from '../Request.jsx'
let bonusItems = [];

function getMigrationID(uid) {
    return uid.toString()
    .replaceAll('1', "A")
    .replaceAll('2', "G")
    .replaceAll('3', "W")
    .replaceAll('4', "Q")
    .replaceAll('5', "Y")
    .replaceAll('6', "6")
    .replaceAll('7', "I")
    .replaceAll('8', "P")
    .replaceAll('9', "U")
    .replaceAll('0', "M")
    + "7";
}

function Bonus() {
    const [inputValue, setInputValue] = useState('');
    const error = useState("");
    
    let itemz = [];
    bonusItems.forEach((e) => {
        itemz.push(e.master_login_bonus_id);
    })
    
    const [submittedItems, setSubmittedItems] = useState(itemz);
    
    const handleSubmit = async (event) => {
        event.preventDefault();
        let input = parseInt(inputValue.trim());
        if (isNaN(input) || submittedItems.includes(input)) return;
        let resp = await Request("/api/webui/startLoginbonus", {
            bonus_id: input
        });
        if (resp.result !== "OK") {
            error[1](resp.message);
            return;
        }
        error[1]("");
        setSubmittedItems([...submittedItems, resp.id]);
        setInputValue('');
    };

    const handleRemoveItem = (index) => {
        const updatedItems = [...submittedItems];
        updatedItems.splice(index, 1);
        setSubmittedItems(updatedItems);
    };
//                <button onClick={() => handleRemoveItem(index)}>X</button>
    return (
        <div>
          <h2>Current login bonus list</h2>
          <div id="error"> { error[0] ? <p>Error: { error[0] } </p> : <p></p> } </div>
          <ul>
            {submittedItems.map((item, index) => (
              <li key={index}>
                {item}
              </li>
            ))}
          </ul>
          <form onSubmit={handleSubmit}>
            <input
              type="text"
              value={inputValue}
              onChange={(event) => setInputValue(event.target.value)}
              placeholder="Enter login bonus ID"
            />
            <button type="submit">Submit</button>
          </form>
          <p>You can find a list of available login bonus IDs <a href="https://github.com/ethanaobrien/ew/blob/main/src/router/databases/json/login_bonus.json">here</a>. You should input the <code>id</code> field</p>
        </div>
    );
}

function Home() {
    const [user, userdata] = useState();
    const [inputValue, setInputValue] = useState('');
    const [serverTime, setServerTime] = useState('');
    const error = useState("");
    
    const logout = () => {
        window.location.href = "/webui/logout";
    }
    const downloadFile = (contents, name) => {
        let a = document.createElement("a");
        a.href = URL.createObjectURL(new Blob([contents], {type: "application/json"}));
        a.download = name;
        a.click();
    }
    const expor = async (e) => {
        e.preventDefault();
        let resp = await Request("/api/webui/export");
        if (resp.result !== "OK") {
            error[1](resp.message);
            return;
        }
        downloadFile(resp.data.userdata, "userdata.json");
        downloadFile(resp.data.userhome, "userhome.json");
        downloadFile(resp.data.missions, "missions.json");
        downloadFile(resp.data.sifcards, "sifcards.json");

    }
    const handleSubmit = async (event) => {
        event.preventDefault();
        let time = Math.round(new Date(inputValue.trim()).getTime() / 1000);
        if (inputValue.trim() === "-1") {
            time = 1711741114;
        } else if (inputValue.trim() === "0") {
            time = 0;
        }
        if (time < 0 || isNaN(time)) return;
        let resp = await Request("/api/webui/set_time", {
            timestamp: time
        });
        if (resp.result !== "OK") {
            error[1](resp.message);
            return;
        }
        error[1]("");
        if (time === 0) {
            setServerTime("now");
        } else {
            setServerTime((new Date(time * 1000)).toString());
        }
        setInputValue('');
    };
    
    useEffect(() => {
        if (user) return;
        (async () => {
            let resp = await Request("/api/webui/userInfo");
            if (resp.result !== "OK") {
                window.location.href = "/?message=" + encodeURIComponent(resp.message);
                return;
            }
            let user = resp.data.userdata;
            bonusItems = resp.data.loginbonus.bonus_list;
            /*
            bonusItems = [{"master_login_bonus_id":1,"day_counts":[1,2],"event_bonus_list":[]}];
            let user = {
                user: {
                    id: 1,
                    rank: 3,
                    exp: 10,
                    last_login_time: 5
                },
                time: new Date()
            }*/
            if (resp.data.time === 0) {
                setServerTime("now");
            } else {
                setServerTime((new Date(resp.data.time * 1000)).toString());
            }
            
            userdata(
                <div>
                    <p>User id: { user.user.id } </p>
                    <p>Migration id: { getMigrationID(user.user.id) } </p>
                    <p>Rank: { user.user.rank } ({ user.user.exp } exp)</p>
                    <p>Last Login: { (new Date(user.user.last_login_time * 1000)).toString() } </p>
                    <Bonus />
                </div>
            );
        })();
    });
  
    return (
        <div id="home">
            <button id="logout" onClick={expor}>Export account</button><br/><br/><br/>
            <button id="logout" onClick={logout}>Logout</button>
            <h1>Home</h1>
            
            { user ? <div> 
                { user } 
                <h2>Server time</h2>
                <div id="error"> { error[0] ? <p>Error: { error[0] } </p> : <p></p> } </div>
                <p>Currently set to { serverTime }. Setting to 0 will set it to now, and -1 will reset it. Time will still progress, based off of when you set this timestamp.</p>
                <form onSubmit={handleSubmit}>
                    <input
                        type="text"
                        value={inputValue}
                        onChange={(event) => setInputValue(event.target.value)}
                        placeholder="Enter Server Time"
                    />
                    <button type="submit">Submit</button>
                </form></div>
            : <p>Loading...</p> }
        </div>
    );
}

export default Home;
